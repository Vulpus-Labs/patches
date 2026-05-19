//! Poly audio-to-trigger edge detector (ADR 0076).
//!
//! Polyphonic variant of [`AudioToTrigger`](super::AudioToTrigger). One
//! independent [`EdgeDetector`] runs per voice channel, so per-voice
//! transients fire on the corresponding lane of the output `PolyTrigger`
//! cable. The fire threshold is `threshold`, never `threshold - hysteresis`
//! — hysteresis controls eligibility (re-arm), never event location. See
//! [`crate::detectors::common::EdgeDetector`] for the kernel.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in` | poly | Per-voice audio source |
//!
//! # Outputs
//!
//! | Port  | Kind          | Description                                       |
//! |-------|---------------|---------------------------------------------------|
//! | `out` | poly trigger  | Per-voice sub-sample sync event (independent lanes) |
//!
//! # Parameters
//!
//! Identical to [`AudioToTrigger`](super::AudioToTrigger); all parameters
//! apply identically across voices.

use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate};
use patches_core::param_frame::ParamView;
use patches_core::{
    module_params, AudioEnvironment, BuildError, CablePool, InputPort, InstanceId, Module,
    ModuleDescriptor, OutputPort, ParameterKind, PolyInput, PolyOutput, StructuralParams,
};

use crate::detectors::audio_to_trigger::EdgeDirectionParam;
use crate::detectors::common::EdgeDetector;

module_params! {
    PolyAudioToTrigger {
        threshold:  Float,
        hysteresis: Float,
        direction:  Enum<EdgeDirectionParam>,
        cooldown:   Float,
    }
}

pub struct PolyAudioToTrigger {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    detectors: [EdgeDetector; 16],
    voice_count: usize,
    in_audio: PolyInput,
    out_trigger: PolyOutput,
}

impl Module for PolyAudioToTrigger {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "PolyAudioToTrigger",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::poly("in")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::poly_trigger("out")],
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
        let detectors = std::array::from_fn(|_| EdgeDetector::new(env.sample_rate));
        Ok(Self {
            instance_id,
            descriptor,
            detectors,
            voice_count: env.poly_voices.min(16),
            in_audio: PolyInput::default(),
            out_trigger: PolyOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        let threshold = p.get(params::threshold);
        let hysteresis = p.get(params::hysteresis);
        let direction = p.get(params::direction).into();
        let cooldown = p.get(params::cooldown);
        for d in &mut self.detectors {
            d.set_threshold_db(threshold);
            d.set_hysteresis_db(hysteresis);
            d.set_direction(direction);
            d.set_cooldown_ms(cooldown);
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
        self.out_trigger = outputs[0].expect_poly_trigger();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let voices = pool.read_poly(&self.in_audio);
        let mut events = [0.0_f32; 16];
        for i in 0..self.voice_count {
            events[i] = self.detectors[i].tick(voices[i]);
        }
        pool.write_poly(&self.out_trigger, events);
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
        ModuleHarness::build_full::<PolyAudioToTrigger>(
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
    fn per_channel_detection_is_independent() {
        // Voice 0 sees a rising crossing; voice 1 stays quiet. Only voice 0's
        // lane fires.
        let mut h = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("hysteresis", ParameterValue::Float(6.0)),
            ("direction", EdgeDirectionParam::Rising.into()),
            ("cooldown", ParameterValue::Float(0.0)),
        ]);
        // Prime: all voices low.
        h.set_poly("in", [0.0; 16]);
        h.tick();
        let _ = h.read_poly("out");

        // Voice 0 transitions high, voice 1 stays low.
        let mut v = [0.0_f32; 16];
        v[0] = 0.8;
        h.set_poly("in", v);
        h.tick();
        let out = h.read_poly("out");
        assert!(out[0] > 0.0, "voice 0 should fire, got {}", out[0]);
        assert_eq!(out[1], 0.0, "voice 1 must not fire when its signal is quiet");
    }

    #[test]
    fn voice_disarm_is_independent_across_channels() {
        // Voice 0 fires (disarms voice 0). Voice 1 then crosses — voice 1
        // must fire because its detector is independent.
        let mut h = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("hysteresis", ParameterValue::Float(6.0)),
            ("direction", EdgeDirectionParam::Rising.into()),
            ("cooldown", ParameterValue::Float(0.0)),
        ]);
        h.set_poly("in", [0.0; 16]);
        h.tick();
        let _ = h.read_poly("out");

        let mut v = [0.0_f32; 16];
        v[0] = 0.8; // voice 0 fires
        h.set_poly("in", v);
        h.tick();
        let out = h.read_poly("out");
        assert!(out[0] > 0.0);

        // Now voice 0 stays high (no refire since voice-0 detector disarmed);
        // voice 1 transitions high — must fire because voice-1 is still armed.
        let mut v2 = [0.0_f32; 16];
        v2[0] = 0.8;
        v2[1] = 0.8;
        h.set_poly("in", v2);
        h.tick();
        let out = h.read_poly("out");
        assert_eq!(out[0], 0.0, "voice 0 must not re-fire while disarmed");
        assert!(out[1] > 0.0, "voice 1 must fire independently, got {}", out[1]);
    }
}
