use patches_core::{
    params_enum,
    AudioEnvironment, AxisId, CablePool, CountAxis, GateInput, InputPort, InstanceId, Module,
    ModuleDescriptor, ModuleDescriptorTemplate, MonoInput, MonoOutput, OutputPort, ParameterKind,
    ParameterTemplate, PortTemplate,
};
use patches_core::{StructuralParams, BuildError};
use patches_core::cables::TriggerInput;
use patches_core::module_params;
use patches_core::param_frame::ParamView;
use patches_dsp::{AdsrCore, AdsrShape};

params_enum! {
    pub enum AdsrShapeParam {
        Linear => "linear",
        Exponential => "exponential",
    }
}

impl From<AdsrShapeParam> for AdsrShape {
    fn from(p: AdsrShapeParam) -> Self {
        match p {
            AdsrShapeParam::Linear => AdsrShape::Linear,
            AdsrShapeParam::Exponential => AdsrShape::Exponential,
        }
    }
}

module_params! {
    Adsr {
        delay:   FloatArray,
        attack:  FloatArray,
        decay:   FloatArray,
        sustain: FloatArray,
        release: FloatArray,
        shape:   EnumArray<AdsrShapeParam>,
    }
}


/// Multi-channel ADSR envelope generator with optional pre-attack delay (DADSR)
/// and built-in VCA pass-through.
///
/// One module emits N independent envelopes (one per channel) driven by a
/// single shared `trigger` and `gate`. With `channels = 1` (the default) it
/// behaves as a conventional single-channel ADSR — `env.out`, `env.vca_in`,
/// `env.vca_out` address channel 0 implicitly. Use `Adsr([amp, filt, vib])`
/// or `Adsr(3)` to get multiple envelopes per trigger/gate.
///
/// Each channel has its own delay/attack/decay/sustain/release/shape, and
/// an optional VCA stage: if `vca_in[c]` is connected, `vca_out[c]` emits
/// `vca_in[c] * env[c]`. Useful for tying amplitude, filter, and
/// modulation envelopes to the same note event without wiring N separate
/// modules.
///
/// A rising `trigger` enters the per-channel Delay phase (output held at
/// 0 for `delay[c]` seconds) before Attack begins. Releasing the gate
/// during Delay returns the channel to Idle without emitting any envelope.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `trigger` | trigger | Shared: one-sample pulse starts the envelope on every channel (ADR 0047) |
/// | `gate` | mono | Shared: held high to sustain; release to enter Release on every channel |
/// | `vca_in[i]` | mono | Optional audio/CV input multiplied by the channel's envelope (i in 0..N-1) |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out[i]` | mono | Envelope level for channel i in \[0.0, 1.0\] |
/// | `vca_out[i]` | mono | `vca_in[i] * out[i]` — pre-multiplied audio/CV |
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
pub struct Adsr {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    channels: usize,
    cores: Vec<AdsrCore>,
    // Port fields
    in_trigger: TriggerInput,
    in_gate: GateInput,
    in_vca: Vec<MonoInput>,
    out_env: Vec<MonoOutput>,
    out_vca: Vec<MonoOutput>,
}

impl Module for Adsr {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "Adsr",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[
                PortTemplate::trigger("trigger"),
                PortTemplate::mono("gate"),
            ],
            per_axis_inputs: &[
                (AxisId::CHANNELS, PortTemplate::mono("vca_in")),
            ],
            global_outputs: &[],
            per_axis_outputs: &[
                (AxisId::CHANNELS, PortTemplate::mono("out")),
                (AxisId::CHANNELS, PortTemplate::mono("vca_out")),
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
                        variants: AdsrShapeParam::VARIANTS,
                        default: "linear",
                    },
                }),
            ],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, _structural: &StructuralParams) -> Result<Self, BuildError> { Ok({
        let channels = descriptor.shape.channels;
        let sr = audio_environment.sample_rate;
        Self {
            instance_id,
            descriptor,
            channels,
            cores: (0..channels).map(|_| AdsrCore::new(sr)).collect(),
            in_trigger: TriggerInput::default(),
            in_gate: GateInput::default(),
            in_vca: vec![MonoInput::default(); channels],
            out_env: vec![MonoOutput::default(); channels],
            out_vca: vec![MonoOutput::default(); channels],
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
            let core = &mut self.cores[c];
            core.set_params(attack, decay, sustain, release);
            core.set_delay(delay);
            core.set_shape(shape);
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        // Input order: trigger(0), gate(1), vca_in[0..n].
        self.in_trigger = TriggerInput::from_ports(inputs, 0);
        self.in_gate = GateInput::from_ports(inputs, 1);
        for c in 0..self.channels {
            self.in_vca[c] = MonoInput::from_ports(inputs, 2 + c);
        }
        // Output order: out[0..n], vca_out[0..n].
        for c in 0..self.channels {
            self.out_env[c] = MonoOutput::from_ports(outputs, c);
            self.out_vca[c] = MonoOutput::from_ports(outputs, self.channels + c);
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let triggered = self.in_trigger.tick(pool).is_some();
        let gate_high = self.in_gate.tick(pool).is_high;
        for c in 0..self.channels {
            let level = self.cores[c].tick(triggered, gate_high);
            let sig = pool.read_mono(&self.in_vca[c]);
            pool.write_mono(&self.out_env[c], level);
            pool.write_mono(&self.out_vca[c], sig * level);
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
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

    fn make_envelope(attack: f32, decay: f32, sustain: f32, release: f32) -> ModuleHarness {
        ModuleHarness::build_full::<Adsr>(
            params!["attack" => attack, "decay" => decay, "sustain" => sustain, "release" => release],
            env_at_10hz(),
            ModuleShape { channels: 1 },
        )
    }

    /// Build a `channels`-wide ADSR with per-channel parameters supplied as
    /// `(name, channel, value)` triples. Defaults apply elsewhere.
    fn make_multi(channels: usize, entries: &[(&str, usize, ParameterValue)]) -> ModuleHarness {
        let mut map = ParameterMap::new();
        for (name, idx, val) in entries {
            map.insert_param(name.to_string(), *idx, val.clone());
        }
        let mut h = ModuleHarness::build_full::<Adsr>(&[], env_at_10hz(), ModuleShape { channels });
        h.update_params_map(&map);
        h
    }

    #[test]
    fn idle_output_is_zero() {
        let mut h = make_envelope(0.5, 0.5, 0.5, 0.5);
        h.set_mono("trigger", 0.0);
        h.set_mono("gate", 0.0);
        h.tick();
        assert_eq!(h.read_mono("out"), 0.0);
        h.tick();
        assert_eq!(h.read_mono("out"), 0.0);
    }

    #[test]
    fn release_falls_to_zero() {
        // attack=0.1s, decay=0.1s, sustain=0.5, release=0.5s (5 samples)
        // release_inc = 0.5 / (0.5 * 10) = 0.1
        let mut h = make_envelope(0.1, 0.1, 0.5, 0.5);
        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        h.tick(); // now in sustain

        h.set_mono("gate", 0.0);
        let expected_release = [0.4, 0.3, 0.2, 0.1, 0.0];
        for &exp in &expected_release {
            h.tick();
            let v = h.read_mono("out");
            assert_within!(exp, v, 1e-5_f32);
        }

        h.tick();
        assert_eq!(h.read_mono("out"), 0.0, "idle after release");
    }

    #[test]
    fn retrigger_mid_release_restarts_attack() {
        let mut h = make_envelope(0.1, 0.1, 0.5, 0.5);
        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        h.tick();
        h.set_mono("gate", 0.0);
        h.tick();
        h.tick();

        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        let v = h.read_mono("out");
        assert_within!(1.0, v, 1e-12_f32);
    }

    /// `vca_out = vca_in * env`. With env at 1.0 (sustain) the VCA passes
    /// the signal through; with env at 0 (idle) the VCA mutes it.
    #[test]
    fn vca_out_multiplies_input_by_envelope() {
        // attack=0.1s (1 sample at 10 Hz, reaches 1.0), sustain=1.0 → env=1.0 after attack.
        let mut h = make_envelope(0.1, 0.1, 1.0, 0.5);
        h.set_mono("vca_in", 0.6);
        // Idle: env=0 → vca_out=0
        h.tick();
        // Actually idle then tick happens before trigger — first sample post-trigger:
        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        // After attack (single sample reaching 1.0) the VCA should pass full input.
        assert_within!(0.6, h.read_mono("vca_out"), 1e-6_f32);
        assert_within!(1.0, h.read_mono("out"), 1e-6_f32);
    }

    /// Channels share `trigger`/`gate` but each has its own envelope and VCA.
    /// Per-channel `attack` differentiates the per-tick envelope values.
    #[test]
    fn multi_channel_shares_trigger_runs_independent_envelopes() {
        // Channel 0: attack=0.1s → 1 sample at 10 Hz → reaches 1.0 immediately.
        // Channel 1: attack=0.5s → 5 samples → 0.2 per sample.
        let mut h = make_multi(2, &[
            ("attack", 0, ParameterValue::Float(0.1)),
            ("attack", 1, ParameterValue::Float(0.5)),
            ("decay",  0, ParameterValue::Float(0.1)),
            ("decay",  1, ParameterValue::Float(0.1)),
            ("sustain",0, ParameterValue::Float(0.5)),
            ("sustain",1, ParameterValue::Float(0.5)),
            ("release",0, ParameterValue::Float(0.5)),
            ("release",1, ParameterValue::Float(0.5)),
        ]);
        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        assert_within!(1.0, h.read_mono_at("out", 0), 1e-6_f32);
        assert_within!(0.2, h.read_mono_at("out", 1), 1e-6_f32);
    }

    /// Per-channel `delay` holds output at 0 for the configured time before
    /// attack begins, even though the trigger fires on both channels at once.
    #[test]
    fn per_channel_delay_holds_independently() {
        let mut h = make_multi(2, &[
            ("delay",  0, ParameterValue::Float(0.0)),
            ("delay",  1, ParameterValue::Float(0.5)),
            ("attack", 0, ParameterValue::Float(0.1)),
            ("attack", 1, ParameterValue::Float(0.1)),
        ]);
        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        assert_within!(1.0, h.read_mono_at("out", 0), 1e-6_f32);
        assert_within!(0.0, h.read_mono_at("out", 1), 1e-6_f32);
        h.set_mono("trigger", 0.0);
        for _ in 0..4 { h.tick(); }
        assert_within!(0.0, h.read_mono_at("out", 1), 1e-6_f32);
        // 6th tick: delay counter exhausts; first attack sample applies.
        h.tick();
        assert!(h.read_mono_at("out", 1) > 0.0, "ch1 attack should begin");
    }
}
