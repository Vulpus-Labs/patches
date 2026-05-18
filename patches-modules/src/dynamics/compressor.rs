//! Feed-forward compressor with sidechain (ADR 0076).
//!
//! Mono variant. The detector reads from `sidechain` when that port is
//! connected, otherwise self-keys from `in`. The DSP kernel lives in
//! [`crate::dynamics::common::CompDetector`] and is tested independently.
//!
//! # Inputs
//!
//! | Port        | Kind | Description                                     |
//! |-------------|------|-------------------------------------------------|
//! | `in`        | mono | Audio input                                     |
//! | `sidechain` | mono | External detector key; self-keys from `in` when unconnected |
//!
//! # Outputs
//!
//! | Port  | Kind | Description     |
//! |-------|------|-----------------|
//! | `out` | mono | Compressed audio |
//!
//! # Parameters
//!
//! | Name         | Type  | Range         | Default | Description                  |
//! |--------------|-------|---------------|---------|------------------------------|
//! | `threshold`  | float | -60..0 dB     | `-12`   | Knee centre                  |
//! | `ratio`      | float | 1..100        | `4`     | Above-knee slope; high ratio approaches limiter |
//! | `knee_width` | float | 0..24 dB      | `6`     | `0` = hard knee              |
//! | `attack`     | float | 0.1..1000 ms  | `10`    | Detector attack              |
//! | `release`    | float | 1..5000 ms    | `100`   | Detector release             |
//! | `makeup`     | float | -24..24 dB    | `0`     | Output gain trim             |
//! | `detect`     | enum  | peak / rms    | `peak`  | Detection mode               |
//! | `mix`        | float | 0..1          | `1`     | Dry/wet blend                |

use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate};
use patches_core::param_frame::ParamView;
use patches_core::{
    module_params, params_enum, AudioEnvironment, BuildError, CablePool, InputPort, InstanceId,
    Module, ModuleDescriptor, MonoInput, MonoOutput, OutputPort, ParameterKind, StructuralParams,
};

use crate::common::sidechain::mono_key;
use crate::dynamics::common::{CompDetector, DetectMode};

params_enum! {
    pub enum CompDetectMode {
        Peak => "peak",
        Rms => "rms",
    }
}

impl From<CompDetectMode> for DetectMode {
    fn from(v: CompDetectMode) -> Self {
        match v {
            CompDetectMode::Peak => DetectMode::Peak,
            CompDetectMode::Rms => DetectMode::Rms,
        }
    }
}

module_params! {
    Compressor {
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

pub struct Compressor {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    detector: CompDetector,
    mix: f32,
    in_audio: MonoInput,
    in_sidechain: MonoInput,
    out_audio: MonoOutput,
}

impl Module for Compressor {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "Compressor",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::mono("in"), PortTemplate::mono("sidechain")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::mono("out")],
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
            mix: 1.0,
            in_audio: MonoInput::default(),
            in_sidechain: MonoInput::default(),
            out_audio: MonoOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.detector.set_threshold_db(p.get(params::threshold));
        self.detector.set_ratio(p.get(params::ratio));
        self.detector.set_knee_width_db(p.get(params::knee_width));
        self.detector.set_attack_ms(p.get(params::attack));
        self.detector.set_release_ms(p.get(params::release));
        self.detector.set_makeup_db(p.get(params::makeup));
        self.detector.set_mode(p.get(params::detect).into());
        self.mix = p.get(params::mix).clamp(0.0, 1.0);
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
        let wet = dry * gain;
        let out = self.mix * wet + (1.0 - self.mix) * dry;
        pool.write_mono(&self.out_audio, out);
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
        ModuleHarness::build_full::<Compressor>(
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
    fn mix_zero_passes_dry_unchanged() {
        let mut h = build(&[
            ("mix", ParameterValue::Float(0.0)),
            ("threshold", ParameterValue::Float(-40.0)),
            ("ratio", ParameterValue::Float(10.0)),
            ("makeup", ParameterValue::Float(0.0)),
        ]);
        // Drive way above threshold; with mix=0 the output must equal input.
        for _ in 0..200 {
            h.set_mono("in", 0.5);
            h.tick();
        }
        let out = h.read_mono("out");
        assert!((out - 0.5).abs() < 1e-5, "mix=0 should pass dry: got {out}");
    }

    #[test]
    fn sidechain_self_key_fallback_matches_self_keyed_input() {
        // Two compressors with identical params. One has nothing on
        // sidechain (self-keys from `in`); the other has the same signal
        // wired into both `in` and `sidechain`. Outputs must agree.
        let mut self_keyed = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("ratio", ParameterValue::Float(4.0)),
            ("knee_width", ParameterValue::Float(0.0)),
            ("attack", ParameterValue::Float(5.0)),
            ("release", ParameterValue::Float(50.0)),
            ("mix", ParameterValue::Float(1.0)),
        ]);
        // Default harness marks every input connected; mimic the
        // unconnected-sidechain wiring used in real patches.
        self_keyed.disconnect_input("sidechain");
        let mut sc_keyed = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("ratio", ParameterValue::Float(4.0)),
            ("knee_width", ParameterValue::Float(0.0)),
            ("attack", ParameterValue::Float(5.0)),
            ("release", ParameterValue::Float(50.0)),
            ("mix", ParameterValue::Float(1.0)),
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
            assert!((a - b).abs() < 1e-4, "self-key vs explicit-sidechain diverged at {i}: {a} vs {b}");
        }
    }

    #[test]
    fn sidechain_external_key_overrides_self_key() {
        // External key keeps the detector busy; the audio input is quiet.
        // With sidechain connected, gain reduction must happen on the
        // quiet input — proving the detector is NOT reading `in`.
        let mut h = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("ratio", ParameterValue::Float(10.0)),
            ("knee_width", ParameterValue::Float(0.0)),
            ("attack", ParameterValue::Float(1.0)),
            ("release", ParameterValue::Float(100.0)),
            ("makeup", ParameterValue::Float(0.0)),
            ("mix", ParameterValue::Float(1.0)),
        ]);
        let signal_amp = 0.1_f32; // -20 dBFS, exactly at threshold; would not compress on its own.
        let key_amp = 1.0_f32;    // 0 dBFS sidechain, drives heavy reduction.
        for _ in 0..4_000 {
            h.set_mono("in", signal_amp);
            h.set_mono("sidechain", key_amp);
            h.tick();
        }
        let out = h.read_mono("out");
        assert!(
            out.abs() < signal_amp * 0.5,
            "external sidechain should attenuate quiet input heavily: out={out}"
        );
    }

    #[test]
    fn peak_and_rms_modes_produce_different_gain_on_transient() {
        // Same impulse-like burst into two compressors differing only in
        // detection mode. The amount of gain reduction must visibly differ.
        let make = |mode: CompDetectMode| {
            build(&[
                ("threshold", ParameterValue::Float(-20.0)),
                ("ratio", ParameterValue::Float(10.0)),
                ("knee_width", ParameterValue::Float(0.0)),
                ("attack", ParameterValue::Float(0.5)),
                ("release", ParameterValue::Float(100.0)),
                ("makeup", ParameterValue::Float(0.0)),
                ("detect", mode.into()),
                ("mix", ParameterValue::Float(1.0)),
            ])
        };
        let mut peak = make(CompDetectMode::Peak);
        let mut rms = make(CompDetectMode::Rms);
        peak.disconnect_input("sidechain");
        rms.disconnect_input("sidechain");

        // 1 ms burst at amplitude 1.0 (well above threshold).
        let burst = (1.0 * 0.001 * SR) as usize;
        let mut last_peak = 0.0;
        let mut last_rms = 0.0;
        for _ in 0..burst {
            peak.set_mono("in", 1.0);
            rms.set_mono("in", 1.0);
            peak.tick();
            rms.tick();
            last_peak = peak.read_mono("out");
            last_rms = rms.read_mono("out");
        }
        assert!(
            (last_peak - last_rms).abs() > 0.005,
            "peak vs rms should differ on a transient: peak={last_peak}, rms={last_rms}"
        );
    }
}
