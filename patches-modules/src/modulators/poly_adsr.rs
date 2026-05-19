use patches_core::{
    params_enum,
    AudioEnvironment, AxisId, CablePool, CountAxis, InputPort, InstanceId, Module,
    ModuleDescriptor, ModuleDescriptorTemplate, OutputPort, ParameterKind, ParameterTemplate,
    PolyGateInput, PolyInput, PolyOutput, PortTemplate,
};
use patches_core::{StructuralParams, BuildError};
use patches_core::cables::PolyTriggerInput;
use patches_core::module_params;
use patches_core::param_frame::ParamView;

params_enum! {
    pub enum PolyAdsrShapeParam {
        Linear => "linear",
        Exponential => "exponential",
    }
}

impl From<PolyAdsrShapeParam> for AdsrShape {
    fn from(p: PolyAdsrShapeParam) -> Self {
        match p {
            PolyAdsrShapeParam::Linear => AdsrShape::Linear,
            PolyAdsrShapeParam::Exponential => AdsrShape::Exponential,
        }
    }
}

module_params! {
    PolyAdsr {
        delay:   FloatArray,
        attack:  FloatArray,
        decay:   FloatArray,
        sustain: FloatArray,
        release: FloatArray,
        shape:   EnumArray<PolyAdsrShapeParam>,
    }
}

use patches_dsp::{AdsrCore, AdsrShape};

/// Multi-channel polyphonic ADSR envelope generator with optional pre-attack
/// delay (DADSR) and built-in VCA pass-through.
///
/// One module emits N independent envelopes per voice (one envelope per
/// channel × per voice), all driven by a single shared poly `trigger` and
/// `gate`. With `channels = 1` (the default) it behaves as a conventional
/// poly ADSR — `env.out`, `env.vca_in`, `env.vca_out` address channel 0
/// implicitly. Use `PolyAdsr([amp, filt, vib])` to ride one trigger/gate
/// across multiple modulation targets without wiring N separate modules.
///
/// Each channel has its own delay/attack/decay/sustain/release/shape, and
/// an optional VCA stage: if `vca_in[c]` (poly) is connected, `vca_out[c]`
/// emits `vca_in[c] * out[c]` per voice. A rising trigger on a voice
/// enters that voice's per-channel Delay phase before Attack begins;
/// releasing the gate during Delay returns the voice/channel to Idle.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `trigger` | poly_trigger | Shared: per-voice rising edge starts the envelope on every channel (ADR 0047) |
/// | `gate` | poly | Shared: per-voice gate; release to enter Release on every channel |
/// | `vca_in[i]` | poly | Optional per-voice audio/CV input multiplied by the channel's envelope |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out[i]` | poly | Envelope level in \[0.0, 1.0\] per voice, per channel |
/// | `vca_out[i]` | poly | `vca_in[i] * out[i]` per voice — pre-multiplied audio/CV |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `delay[i]` | float | 0.0 -- 10.0 | `0.0` | Pre-attack hold time in seconds (output stays at 0) |
/// | `attack[i]` | float | 0.001 -- 10.0 | `0.01` | Attack time in seconds |
/// | `decay[i]` | float | 0.001 -- 10.0 | `0.1` | Decay time in seconds |
/// | `sustain[i]` | float | 0.0 -- 1.0 | `0.7` | Sustain level |
/// | `release[i]` | float | 0.001 -- 10.0 | `0.3` | Release time in seconds |
/// | `shape[i]` | enum | linear, exponential | `linear` | Segment shape: linear ramp (default) or analog-style RC curve |
pub struct PolyAdsr {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    channels: usize,
    /// `voices[channel][voice]`.
    voices: Vec<[AdsrCore; 16]>,
    in_trigger: PolyTriggerInput,
    in_gate: PolyGateInput,
    in_vca: Vec<PolyInput>,
    out_env: Vec<PolyOutput>,
    out_vca: Vec<PolyOutput>,
}

impl Module for PolyAdsr {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "PolyAdsr",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[
                PortTemplate::poly_trigger("trigger"),
                PortTemplate::poly("gate"),
            ],
            per_axis_inputs: &[
                (AxisId::CHANNELS, PortTemplate::poly("vca_in")),
            ],
            global_outputs: &[],
            per_axis_outputs: &[
                (AxisId::CHANNELS, PortTemplate::poly("out")),
                (AxisId::CHANNELS, PortTemplate::poly("vca_out")),
            ],
            realtime_params: &[],
            structural_params: &[],
            per_axis_realtime_params: &[
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::delay.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 10.0, default: 0.0 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::attack.as_str(),
                    kind: ParameterKind::Float { min: 0.001, max: 10.0, default: 0.01 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::decay.as_str(),
                    kind: ParameterKind::Float { min: 0.001, max: 10.0, default: 0.1 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::sustain.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.7 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::release.as_str(),
                    kind: ParameterKind::Float { min: 0.001, max: 10.0, default: 0.3 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::shape.as_str(),
                    kind: ParameterKind::Enum {
                        variants: PolyAdsrShapeParam::VARIANTS,
                        default: "linear",
                    },
                }),
            ],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, _structural: &StructuralParams) -> Result<Self, BuildError> { Ok({
        let sr = audio_environment.sample_rate;
        let channels = descriptor.shape.channels;
        Self {
            instance_id,
            descriptor,
            channels,
            voices: (0..channels).map(|_| std::array::from_fn(|_| AdsrCore::new(sr))).collect(),
            in_trigger: PolyTriggerInput::default(),
            in_gate: PolyGateInput::default(),
            in_vca: vec![PolyInput::default(); channels],
            out_env: vec![PolyOutput::default(); channels],
            out_vca: vec![PolyOutput::default(); channels],
        }
    })}

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        for c in 0..self.channels {
            let idx = c as u16;
            let attack = p.get(params::attack.at(idx));
            let decay = p.get(params::decay.at(idx));
            let sustain = p.get(params::sustain.at(idx));
            let release = p.get(params::release.at(idx));
            let delay = p.get(params::delay.at(idx));
            let shape: AdsrShape = p.get(params::shape.at(idx)).into();
            for voice in &mut self.voices[c] {
                voice.set_params(attack, decay, sustain, release);
                voice.set_delay(delay);
                voice.set_shape(shape);
            }
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        // Input order: trigger(0), gate(1), vca_in[0..n].
        self.in_trigger = PolyTriggerInput::from_ports(inputs, 0);
        self.in_gate    = PolyGateInput::from_ports(inputs, 1);
        for c in 0..self.channels {
            self.in_vca[c] = PolyInput::from_ports(inputs, 2 + c);
        }
        // Output order: out[0..n], vca_out[0..n].
        for c in 0..self.channels {
            self.out_env[c] = PolyOutput::from_ports(outputs, c);
            self.out_vca[c] = PolyOutput::from_ports(outputs, self.channels + c);
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let triggers = self.in_trigger.tick(pool);
        let gates    = self.in_gate.tick(pool);

        for c in 0..self.channels {
            let vca_in = pool.read_poly(&self.in_vca[c]);
            let mut env_out = [0.0f32; 16];
            let mut vca_out = [0.0f32; 16];
            for v in 0..16 {
                let level = self.voices[c][v].tick(triggers[v].is_some(), gates[v].is_high);
                env_out[v] = level;
                vca_out[v] = vca_in[v] * level;
            }
            pool.write_poly(&self.out_env[c], env_out);
            pool.write_poly(&self.out_vca[c], vca_out);
        }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{AudioEnvironment, ModuleShape};
    use patches_core::parameter_map::{ParameterMap, ParameterValue};
    use patches_core::test_support::{assert_within, ModuleHarness, params};

    fn env_at_10hz() -> AudioEnvironment {
        AudioEnvironment { sample_rate: 10.0, poly_voices: 16, periodic_update_interval: 32, hosted: false }
    }

    fn make_adsr(attack: f32, decay: f32, sustain: f32, release: f32, _voices: usize) -> ModuleHarness {
        ModuleHarness::build_full::<PolyAdsr>(
            params!["attack" => attack, "decay" => decay, "sustain" => sustain, "release" => release],
            env_at_10hz(),
            ModuleShape { channels: 1 },
        )
    }

    fn make_multi(channels: usize, entries: &[(&str, usize, ParameterValue)]) -> ModuleHarness {
        let mut map = ParameterMap::new();
        for (name, idx, val) in entries {
            map.insert_param(name.to_string(), *idx, val.clone());
        }
        let mut h = ModuleHarness::build_full::<PolyAdsr>(&[], env_at_10hz(), ModuleShape { channels });
        h.update_params_map(&map);
        h
    }

    fn arr(val: f32, voice: usize) -> [f32; 16] {
        let mut a = [0.0f32; 16];
        a[voice] = val;
        a
    }

    #[test]
    fn idle_output_is_zero() {
        let mut h = make_adsr(0.5, 0.5, 0.5, 0.5, 2);
        h.set_poly("trigger", [0.0; 16]);
        h.set_poly("gate",    [0.0; 16]);
        h.tick();
        let out = h.read_poly("out");
        assert_eq!(out[0], 0.0);
        assert_eq!(out[1], 0.0);
    }

    #[test]
    fn attack_rises_on_trigger_for_single_voice() {
        // attack=0.5s at 10Hz → 5 samples, inc=0.2
        let mut h = make_adsr(0.5, 1.0, 0.5, 0.5, 2);
        h.set_poly("trigger", arr(1.0, 0));
        h.set_poly("gate",    arr(1.0, 0));
        h.tick();
        let out = h.read_poly("out");
        assert_within!(0.2, out[0], 1e-12_f32);
        assert_eq!(out[1], 0.0);
    }

    #[test]
    fn sustain_holds_while_gate_high() {
        let mut h = make_adsr(0.1, 0.1, 0.6, 1.0, 2);
        h.set_poly("trigger", arr(1.0, 0));
        h.set_poly("gate",    arr(1.0, 0));
        h.tick();
        h.set_poly("trigger", arr(0.0, 0));
        h.tick();

        for _ in 0..5 {
            h.tick();
            assert_within!(0.6, h.read_poly_voice("out", 0), 1e-12_f32);
            assert_eq!(h.read_poly_voice("out", 1), 0.0, "voice 1 must remain silent");
        }
    }

    #[test]
    fn release_falls_to_zero() {
        let mut h = make_adsr(0.1, 0.1, 0.5, 0.5, 2);
        h.set_poly("trigger", arr(1.0, 0));
        h.set_poly("gate",    arr(1.0, 0));
        h.tick();
        h.set_poly("trigger", arr(0.0, 0));
        h.tick();

        h.set_poly("gate", arr(0.0, 0));
        let expected_release = [0.4_f32, 0.3, 0.2, 0.1, 0.0];
        for &exp in &expected_release {
            h.tick();
            assert_within!(exp, h.read_poly_voice("out", 0), 1e-5_f32);
            assert_eq!(h.read_poly_voice("out", 1), 0.0, "voice 1 must remain silent");
        }
        h.tick();
        assert_eq!(h.read_poly_voice("out", 0), 0.0, "idle after release");
    }

    #[test]
    fn retrigger_mid_release_restarts_attack() {
        let mut h = make_adsr(0.1, 0.1, 0.5, 0.5, 2);
        h.set_poly("trigger", arr(1.0, 0));
        h.set_poly("gate",    arr(1.0, 0));
        h.tick();
        h.set_poly("trigger", arr(0.0, 0));
        h.tick();
        h.set_poly("gate", arr(0.0, 0));
        h.tick();
        h.tick();

        h.set_poly("trigger", arr(1.0, 0));
        h.set_poly("gate",    arr(1.0, 0));
        h.tick();
        assert_within!(1.0, h.read_poly_voice("out", 0), 1e-12_f32);
        assert_eq!(h.read_poly_voice("out", 1), 0.0, "voice 1 must remain silent");
    }

    #[test]
    fn poly_output_clamped_to_unit_range() {
        let mut h = make_adsr(0.1, 0.1, 0.5, 0.5, 2);
        let mut trigger = [0.0f32; 16];
        trigger[0] = 1.0;
        trigger[1] = 1.0;
        let mut gate = [0.0f32; 16];
        gate[0] = 1.0;
        gate[1] = 1.0;
        h.set_poly("trigger", trigger);
        h.set_poly("gate",    gate);
        for _ in 0..20 {
            h.tick();
            let out = h.read_poly("out");
            for (i, &v) in out[..2].iter().enumerate() {
                assert!((0.0..=1.0).contains(&v), "voice {i} output out of [0, 1]: {v}");
            }
        }
    }

    #[test]
    fn two_voices_are_independent() {
        let mut h = make_adsr(0.1, 0.5, 0.5, 0.5, 2);
        h.set_poly("trigger", arr(1.0, 0));
        h.set_poly("gate",    arr(1.0, 0));
        h.tick();
        let mut trig1 = [0.0f32; 16];
        trig1[1] = 1.0;
        let mut gate1 = [0.0f32; 16];
        gate1[0] = 1.0;
        gate1[1] = 1.0;
        h.set_poly("trigger", trig1);
        h.set_poly("gate",    gate1);
        h.tick();
        let out = h.read_poly("out");
        assert_within!(0.9, out[0], 1e-10_f32);
        assert_within!(1.0, out[1], 1e-10_f32);
    }

    /// `vca_out` per-voice = `vca_in * env` per-voice. With env at 1.0 the
    /// VCA passes the signal through.
    #[test]
    fn vca_out_multiplies_input_by_envelope_per_voice() {
        let mut h = make_adsr(0.1, 0.1, 1.0, 0.5, 1);
        let mut vca_in = [0.0f32; 16];
        vca_in[0] = 0.6;
        vca_in[1] = -0.4;
        h.set_poly("vca_in", vca_in);
        h.set_poly("trigger", {
            let mut a = [0.0f32; 16]; a[0] = 1.0; a[1] = 1.0; a
        });
        h.set_poly("gate", {
            let mut a = [0.0f32; 16]; a[0] = 1.0; a[1] = 1.0; a
        });
        h.tick();
        assert_within!(0.6,  h.read_poly_voice("vca_out", 0), 1e-6_f32);
        assert_within!(-0.4, h.read_poly_voice("vca_out", 1), 1e-6_f32);
    }

    /// Two channels of the same poly ADSR share trigger/gate but apply
    /// independent attack times.
    #[test]
    fn multi_channel_shares_poly_trigger() {
        let mut h = make_multi(2, &[
            ("attack", 0, ParameterValue::Float(0.1)),
            ("attack", 1, ParameterValue::Float(0.5)),
            ("decay",  0, ParameterValue::Float(0.1)),
            ("decay",  1, ParameterValue::Float(0.1)),
            ("sustain",0, ParameterValue::Float(0.5)),
            ("sustain",1, ParameterValue::Float(0.5)),
        ]);
        h.set_poly("trigger", arr(1.0, 0));
        h.set_poly("gate",    arr(1.0, 0));
        h.tick();
        // Channel 0 reaches 1.0 in one sample on voice 0.
        assert_within!(1.0, h.read_poly_voice_at("out", 0, 0), 1e-6_f32);
        // Channel 1 attack inc is 0.2.
        assert_within!(0.2, h.read_poly_voice_at("out", 1, 0), 1e-6_f32);
    }
}
