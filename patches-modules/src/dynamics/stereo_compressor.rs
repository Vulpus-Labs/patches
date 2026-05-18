//! True-stereo feed-forward compressor with linked detection (ADR 0076).
//!
//! Stereo variant of [`Compressor`](super::compressor::Compressor). One
//! detector is fed by a single magnitude derived from both channels:
//!
//! - **Peak:** `max(|L|, |R|)`
//! - **RMS:**  `sqrt((L² + R²) / 2)`
//!
//! A single gain-reduction value is applied identically to both channels,
//! preserving the stereo image under asymmetric transients.
//!
//! # Sidechain
//!
//! The `sidechain` port is stereo. When unconnected the detector self-keys
//! from `in`. A mono source patched into the stereo `sidechain` port is
//! broadcast to both channels automatically (ADR 0059).
//!
//! # Inputs
//!
//! | Port        | Kind   | Description                                |
//! |-------------|--------|--------------------------------------------|
//! | `in`        | stereo | Audio input                                |
//! | `sidechain` | stereo | External detector key; self-keys from `in` when unconnected |
//!
//! # Outputs
//!
//! | Port  | Kind   | Description       |
//! |-------|--------|-------------------|
//! | `out` | stereo | Compressed stereo |
//!
//! # Parameters
//!
//! Identical to [`Compressor`](super::compressor::Compressor).

use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate};
use patches_core::param_frame::ParamView;
use patches_core::{
    module_params, AudioEnvironment, BuildError, CablePool, InputPort, InstanceId, Module,
    ModuleDescriptor, OutputPort, ParameterKind, StereoInput, StereoOutput, StructuralParams,
};

use crate::common::sidechain::stereo_key;
use crate::dynamics::common::{CompDetector, DetectMode};
use crate::dynamics::compressor::CompDetectMode;

module_params! {
    StereoCompressor {
        threshold:  Float,
        ratio:      Float,
        knee_width: Float,
        attack:     Float,
        release:    Float,
        makeup:     Float,
        detect:     Enum<CompDetectMode>,
        mix:        Float,
    }
}

pub struct StereoCompressor {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    detector: CompDetector,
    mode: DetectMode,
    mix: f32,
    in_audio: StereoInput,
    in_sidechain: StereoInput,
    out_audio: StereoOutput,
}

impl Module for StereoCompressor {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "StereoCompressor",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::stereo("in"), PortTemplate::stereo("sidechain")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::stereo("out")],
            per_axis_outputs: &[],
            realtime_params: &[
                ParameterTemplate {
                    name: params::threshold.as_str(),
                    kind: ParameterKind::Float { min: -60.0, max: 0.0, default: -12.0 },
                },
                ParameterTemplate {
                    name: params::ratio.as_str(),
                    kind: ParameterKind::Float { min: 1.0, max: 100.0, default: 4.0 },
                },
                ParameterTemplate {
                    name: params::knee_width.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 24.0, default: 6.0 },
                },
                ParameterTemplate {
                    name: params::attack.as_str(),
                    kind: ParameterKind::Float { min: 0.1, max: 1000.0, default: 10.0 },
                },
                ParameterTemplate {
                    name: params::release.as_str(),
                    kind: ParameterKind::Float { min: 1.0, max: 5000.0, default: 100.0 },
                },
                ParameterTemplate {
                    name: params::makeup.as_str(),
                    kind: ParameterKind::Float { min: -24.0, max: 24.0, default: 0.0 },
                },
                ParameterTemplate {
                    name: params::detect.as_str(),
                    kind: ParameterKind::Enum {
                        variants: CompDetectMode::VARIANTS,
                        default: "peak",
                    },
                },
                ParameterTemplate {
                    name: params::mix.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 1.0 },
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
            detector: CompDetector::new(env.sample_rate),
            mode: DetectMode::Peak,
            mix: 1.0,
            in_audio: StereoInput::default(),
            in_sidechain: StereoInput::default(),
            out_audio: StereoOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.detector.set_threshold_db(p.get(params::threshold));
        self.detector.set_ratio(p.get(params::ratio));
        self.detector.set_knee_width_db(p.get(params::knee_width));
        self.detector.set_attack_ms(p.get(params::attack));
        self.detector.set_release_ms(p.get(params::release));
        self.detector.set_makeup_db(p.get(params::makeup));
        self.mode = p.get(params::detect).into();
        self.detector.set_mode(self.mode);
        self.mix = p.get(params::mix).clamp(0.0, 1.0);
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
        let mag = match self.mode {
            DetectMode::Peak => k_l.abs().max(k_r.abs()),
            DetectMode::Rms => (0.5 * (k_l * k_l + k_r * k_r)).sqrt(),
        };
        let gain = self.detector.tick(mag);
        let wet_l = dry_l * gain;
        let wet_r = dry_r * gain;
        let out_l = self.mix * wet_l + (1.0 - self.mix) * dry_l;
        let out_r = self.mix * wet_r + (1.0 - self.mix) * dry_r;
        pool.write_stereo(&self.out_audio, out_l, out_r);
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
        ModuleHarness::build_full::<StereoCompressor>(
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
    fn asymmetric_lr_transient_applies_same_gain_to_both_channels() {
        // L is loud (drives the detector), R is constantly half-amplitude.
        // The linked-detector gain must apply equally to both — measured as
        // identical out/in ratio on each channel.
        let mut h = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("ratio", ParameterValue::Float(8.0)),
            ("knee_width", ParameterValue::Float(0.0)),
            ("attack", ParameterValue::Float(1.0)),
            ("release", ParameterValue::Float(100.0)),
            ("makeup", ParameterValue::Float(0.0)),
            ("mix", ParameterValue::Float(1.0)),
        ]);
        h.disconnect_input("sidechain");

        let l = 1.0_f32;
        let r = 0.4_f32;
        for _ in 0..4_000 {
            h.set_stereo("in", l, r);
            h.tick();
        }
        let (out_l, out_r) = h.read_stereo("out");
        let g_l = out_l / l;
        let g_r = out_r / r;
        assert!(
            (g_l - g_r).abs() < 1.0e-4,
            "linked detector must apply identical gain: g_l={g_l}, g_r={g_r}"
        );
        // Sanity: real reduction is happening, not a no-op.
        assert!(g_l < 0.9, "no gain reduction observed (g_l={g_l})");
    }

    #[test]
    fn external_sidechain_overrides_self_key_in_stereo() {
        let mut h = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("ratio", ParameterValue::Float(10.0)),
            ("knee_width", ParameterValue::Float(0.0)),
            ("attack", ParameterValue::Float(1.0)),
            ("release", ParameterValue::Float(100.0)),
            ("makeup", ParameterValue::Float(0.0)),
            ("mix", ParameterValue::Float(1.0)),
        ]);

        // Quiet audio, loud sidechain on both channels.
        for _ in 0..4_000 {
            h.set_stereo("in", 0.1, 0.1);
            h.set_stereo("sidechain", 1.0, 1.0);
            h.tick();
        }
        let (out_l, out_r) = h.read_stereo("out");
        assert!(
            out_l.abs() < 0.06 && out_r.abs() < 0.06,
            "external sidechain should attenuate quiet input: ({out_l}, {out_r})"
        );
    }

    #[test]
    fn mix_zero_passes_dry_stereo_unchanged() {
        let mut h = build(&[
            ("mix", ParameterValue::Float(0.0)),
            ("threshold", ParameterValue::Float(-40.0)),
            ("ratio", ParameterValue::Float(10.0)),
        ]);
        for _ in 0..200 {
            h.set_stereo("in", 0.7, 0.3);
            h.tick();
        }
        let (l, r) = h.read_stereo("out");
        assert!((l - 0.7).abs() < 1e-5, "L dry passthrough: {l}");
        assert!((r - 0.3).abs() < 1e-5, "R dry passthrough: {r}");
    }
}
