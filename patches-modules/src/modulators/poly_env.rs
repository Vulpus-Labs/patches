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
use patches_dsp::{AdsrShape, EnvCore, Stage, MAX_STAGES};

params_enum! {
    pub enum PolyEnvCurveParam {
        Linear => "linear",
        Exponential => "exponential",
    }
}

impl From<PolyEnvCurveParam> for AdsrShape {
    fn from(p: PolyEnvCurveParam) -> Self {
        match p {
            PolyEnvCurveParam::Linear => AdsrShape::Linear,
            PolyEnvCurveParam::Exponential => AdsrShape::Exponential,
        }
    }
}

module_params! {
    PolyEnv {
        time:      FloatArray,
        level:     FloatArray,
        curve:     EnumArray<PolyEnvCurveParam>,
        keyfollow: Float,
        ref_key:   Float,
        vel_depth: Float,
    }
}

/// 16-voice multi-stage breakpoint envelope with key-follow time-scaling,
/// velocity scaling, and a built-in VCA pass-through — the poly form of `Env`.
///
/// Like `Env`, it runs an arbitrary number of `(time, level, curve)` stages with
/// a designated sustain stage; the stage count is the module's *channels* axis
/// (`PolyEnv(5)` = a five-stage envelope). Every voice shares the same stage
/// configuration but holds independent state, driven by per-voice poly
/// `trigger`, `gate`, `voct`, and `velocity` cables.
///
/// Stages `0..sustain_stage` form the pre-sustain contour; each voice holds at
/// `level[sustain_stage]` while its gate is high, then runs the remaining stages
/// `sustain_stage+1..N` as the release tail on gate-off. Designate a final stage
/// with `level = 0` for a release to silence. A re-trigger restarts from the
/// voice's current level (no click), and a gate-off during the pre-sustain
/// contour jumps straight into the release tail.
///
/// **Key-follow** shortens stage times with pitch per voice:
/// `time_scale = 2^(-keyfollow * (voct[v] - ref_key))`, applied per tick, so a
/// bending `voct` re-scales that voice's contour live. **Velocity** scales stage
/// *levels* per voice via `1 - vel_depth * (1 - velocity[v])`, latched at each
/// voice's trigger. Unconnected `voct` reads as 0 and unconnected `velocity` as
/// 1.0 (no attenuation).
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `trigger` | poly_trigger | Per-voice rising edge starts the envelope (ADR 0047) |
/// | `gate` | poly | Per-voice gate; release to run the release tail |
/// | `voct` | poly | Per-voice 1V/oct pitch driving key-follow (0 if unconnected) |
/// | `velocity` | poly | Per-voice velocity in \[0, 1\] scaling stage levels (1.0 if unconnected) |
/// | `vca_in` | poly | Optional per-voice audio/CV input multiplied by the envelope |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | poly | Per-voice envelope level in \[0.0, 1.0\] |
/// | `vca_out` | poly | `vca_in * out` per voice — pre-multiplied audio/CV |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `time[i]` | float | 0.0 -- 10.0 | `0.1` | Stage i duration in seconds (i in 0..N−1, N = stage count) |
/// | `level[i]` | float | 0.0 -- 1.0 | `1.0` | Stage i target level |
/// | `curve[i]` | enum | linear, exponential | `linear` | Stage i segment shape |
/// | `sustain_stage` | int (structural) | 0 -- 7 | `0` | Index of the held stage; later stages are the release tail |
/// | `keyfollow` | float | 0.0 -- 1.0 | `0.0` | Pitch time-scaling depth (1.0 halves times one octave up) |
/// | `ref_key` | float | -5.0 -- 5.0 | `0.0` | Reference pitch (V/oct) at which `time_scale = 1` |
/// | `vel_depth` | float | 0.0 -- 1.0 | `0.0` | How much velocity attenuates stage levels |
pub struct PolyEnv {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    /// Number of stages = the channels axis, capped at [`MAX_STAGES`].
    stages: usize,
    /// Structural: index of the held sustain stage.
    sustain_stage: usize,
    /// One independent envelope per voice; all share the same stage config.
    voices: [EnvCore; 16],
    keyfollow: f32,
    ref_key: f32,
    vel_depth: f32,
    in_trigger: PolyTriggerInput,
    in_gate: PolyGateInput,
    in_voct: PolyInput,
    in_velocity: PolyInput,
    in_vca: PolyInput,
    out_env: PolyOutput,
    out_vca: PolyOutput,
}

impl Module for PolyEnv {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "PolyEnv",
            // The channels axis is the stage count for this module.
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[
                PortTemplate::poly_trigger("trigger"),
                PortTemplate::poly("gate"),
                PortTemplate::poly("voct"),
                PortTemplate::poly("velocity"),
                PortTemplate::poly("vca_in"),
            ],
            per_axis_inputs: &[],
            global_outputs: &[
                PortTemplate::poly("out"),
                PortTemplate::poly("vca_out"),
            ],
            per_axis_outputs: &[],
            realtime_params: &[
                ParameterTemplate {
                    name: params::keyfollow.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.0 },
                },
                ParameterTemplate {
                    name: params::ref_key.as_str(),
                    kind: ParameterKind::Float { min: -5.0, max: 5.0, default: 0.0 },
                },
                ParameterTemplate {
                    name: params::vel_depth.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.0 },
                },
            ],
            structural_params: &[
                ParameterTemplate {
                    name: "sustain_stage",
                    kind: ParameterKind::Int { min: 0, max: (MAX_STAGES - 1) as i64, default: 0 },
                },
            ],
            per_axis_realtime_params: &[
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::time.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 10.0, default: 0.1 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::level.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 1.0 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::curve.as_str(),
                    kind: ParameterKind::Enum {
                        variants: PolyEnvCurveParam::VARIANTS,
                        default: "linear",
                    },
                }),
            ],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, structural: &StructuralParams) -> Result<Self, BuildError> { Ok({
        let sr = audio_environment.sample_rate;
        let stages = (descriptor.shape.channels).min(MAX_STAGES);
        let sustain_stage = structural
            .get_int("sustain_stage", 0)
            .unwrap_or(0)
            .clamp(0, (MAX_STAGES - 1) as i64) as usize;
        Self {
            instance_id,
            descriptor,
            stages,
            sustain_stage,
            voices: std::array::from_fn(|_| EnvCore::new(sr)),
            keyfollow: 0.0,
            ref_key: 0.0,
            vel_depth: 0.0,
            in_trigger: PolyTriggerInput::default(),
            in_gate: PolyGateInput::default(),
            in_voct: PolyInput::default(),
            in_velocity: PolyInput::default(),
            in_vca: PolyInput::default(),
            out_env: PolyOutput::default(),
            out_vca: PolyOutput::default(),
        }
    })}

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.keyfollow = p.get(params::keyfollow);
        self.ref_key = p.get(params::ref_key);
        self.vel_depth = p.get(params::vel_depth);

        // Build the stage list once on the stack (no allocation) and apply it to
        // every voice; re-apply the sustain index (set_stages re-clamps it).
        let mut stages = [Stage::default(); MAX_STAGES];
        for c in 0..self.stages {
            let idx = c as u16;
            stages[c] = Stage::new(
                p.get(params::level.at(idx)),
                p.get(params::time.at(idx)),
                p.get(params::curve.at(idx)).into(),
            );
        }
        for voice in &mut self.voices {
            voice.set_stages(&stages[..self.stages]);
            voice.set_sustain_stage(self.sustain_stage);
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        // Input order: trigger(0), gate(1), voct(2), velocity(3), vca_in(4).
        self.in_trigger = PolyTriggerInput::from_ports(inputs, 0);
        self.in_gate = PolyGateInput::from_ports(inputs, 1);
        self.in_voct = PolyInput::from_ports(inputs, 2);
        self.in_velocity = PolyInput::from_ports(inputs, 3);
        self.in_vca = PolyInput::from_ports(inputs, 4);
        // Output order: out(0), vca_out(1).
        self.out_env = PolyOutput::from_ports(outputs, 0);
        self.out_vca = PolyOutput::from_ports(outputs, 1);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let triggers = self.in_trigger.tick(pool);
        let gates = self.in_gate.tick(pool);

        let voct = if self.in_voct.is_connected() {
            pool.read_poly(&self.in_voct)
        } else {
            [0.0; 16]
        };
        let velocity = if self.in_velocity.is_connected() {
            pool.read_poly(&self.in_velocity)
        } else {
            [1.0; 16]
        };
        let vca_in = pool.read_poly(&self.in_vca);

        let mut env_out = [0.0f32; 16];
        let mut vca_out = [0.0f32; 16];
        for v in 0..16 {
            self.voices[v]
                .set_level_scale(1.0 - self.vel_depth * (1.0 - velocity[v].clamp(0.0, 1.0)));
            let time_scale = (-self.keyfollow * (voct[v] - self.ref_key)).exp2();
            let level = self.voices[v].tick(triggers[v].is_some(), gates[v].is_high, time_scale);
            env_out[v] = level;
            vca_out[v] = vca_in[v] * level;
        }
        if self.out_env.is_connected() {
            pool.write_poly(&self.out_env, env_out);
        }
        if self.out_vca.is_connected() {
            pool.write_poly(&self.out_vca, vca_out);
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

    fn make_single(entries: &[(&str, ParameterValue)]) -> ModuleHarness {
        ModuleHarness::build_full::<PolyEnv>(entries, env_at_10hz(), ModuleShape { channels: 1 })
    }

    fn make_stages(n: usize, entries: &[(&str, usize, ParameterValue)]) -> ModuleHarness {
        let mut map = ParameterMap::new();
        for (name, idx, val) in entries {
            map.insert_param(name.to_string(), *idx, val.clone());
        }
        let mut h = ModuleHarness::build_full::<PolyEnv>(&[], env_at_10hz(), ModuleShape { channels: n });
        h.update_params_map(&map);
        h
    }

    /// Set `val` on `voice`, leaving other lanes at 0.
    fn arr(val: f32, voice: usize) -> [f32; 16] {
        let mut a = [0.0f32; 16];
        a[voice] = val;
        a
    }

    #[test]
    fn idle_output_is_zero() {
        let mut h = make_single(params!["time" => 0.5_f32, "level" => 1.0_f32]);
        h.set_poly("trigger", [0.0; 16]);
        h.set_poly("gate", [0.0; 16]);
        h.tick();
        let out = h.read_poly("out");
        assert_eq!(out[0], 0.0);
        assert_eq!(out[3], 0.0);
    }

    /// Per-voice key-follow: at `keyfollow = 1.0`, a voice one octave up
    /// advances twice as fast as a voice at the reference pitch.
    #[test]
    fn keyfollow_is_per_voice() {
        // 1 stage, time 0.4s (4 samples at 10 Hz), ramp to 1.0, keyfollow 1.0.
        let mut h = make_single(params![
            "time" => 0.4_f32, "level" => 1.0_f32, "keyfollow" => 1.0_f32
        ]);
        // Voice 0 at ref (voct 0), voice 1 one octave up (voct 1).
        let mut voct = [0.0f32; 16];
        voct[1] = 1.0;
        h.set_poly("voct", voct);
        let mut trig = [0.0f32; 16];
        trig[0] = 1.0;
        trig[1] = 1.0;
        h.set_poly("trigger", trig);
        h.set_poly("gate", trig);
        h.tick();
        // Voice 0: inc 0.25. Voice 1: time halved → inc 0.5.
        assert_within!(0.25, h.read_poly_voice("out", 0), 1e-6_f32);
        assert_within!(0.5, h.read_poly_voice("out", 1), 1e-6_f32);
    }

    /// Per-voice velocity scaling: `vel_depth = 1.0`, voice with velocity 0.5
    /// holds at half level; unconnected lanes still need an explicit velocity
    /// value here since the input is connected.
    #[test]
    fn velocity_scales_level_per_voice() {
        // 1 stage, instant to 1.0, sustain at stage 0.
        let mut h = make_single(params![
            "time" => 0.0_f32, "level" => 1.0_f32, "vel_depth" => 1.0_f32
        ]);
        let mut vel = [0.0f32; 16];
        vel[0] = 1.0;
        vel[1] = 0.5;
        h.set_poly("velocity", vel);
        let mut trig = [0.0f32; 16];
        trig[0] = 1.0;
        trig[1] = 1.0;
        h.set_poly("trigger", trig);
        h.set_poly("gate", trig);
        h.tick();
        assert_within!(1.0, h.read_poly_voice("out", 0), 1e-6_f32);
        assert_within!(0.5, h.read_poly_voice("out", 1), 1e-6_f32);
    }

    /// Unconnected velocity behaves as full velocity across all voices.
    #[test]
    fn unconnected_velocity_is_full() {
        let mut h = make_single(params![
            "time" => 0.0_f32, "level" => 1.0_f32, "vel_depth" => 1.0_f32
        ]);
        h.disconnect_input("velocity");
        let mut trig = [0.0f32; 16];
        trig[0] = 1.0;
        h.set_poly("trigger", trig);
        h.set_poly("gate", trig);
        h.tick();
        assert_within!(1.0, h.read_poly_voice("out", 0), 1e-6_f32);
    }

    /// VCA pass-through per voice equals `vca_in * env`.
    #[test]
    fn vca_out_equals_input_times_env_per_voice() {
        let mut h = make_single(params!["time" => 0.0_f32, "level" => 1.0_f32]);
        let mut vca_in = [0.0f32; 16];
        vca_in[0] = 0.6;
        vca_in[1] = -0.4;
        h.set_poly("vca_in", vca_in);
        let mut trig = [0.0f32; 16];
        trig[0] = 1.0;
        trig[1] = 1.0;
        h.set_poly("trigger", trig);
        h.set_poly("gate", trig);
        h.tick();
        // Instant stage → env = 1.0 on both voices.
        assert_within!(0.6, h.read_poly_voice("vca_out", 0), 1e-6_f32);
        assert_within!(-0.4, h.read_poly_voice("vca_out", 1), 1e-6_f32);
    }

    /// Voices are independent: triggering voice 0 leaves voice 1 silent.
    #[test]
    fn voices_are_independent() {
        let mut h = make_single(params!["time" => 0.0_f32, "level" => 1.0_f32]);
        h.set_poly("trigger", arr(1.0, 0));
        h.set_poly("gate", arr(1.0, 0));
        h.tick();
        assert_within!(1.0, h.read_poly_voice("out", 0), 1e-6_f32);
        assert_eq!(h.read_poly_voice("out", 1), 0.0, "voice 1 must remain silent");
    }

    /// Two-stage envelope (sustain stage 0, release stage 1) holds under gate,
    /// then runs the release tail on a single voice.
    #[test]
    fn sustain_then_release_tail() {
        let mut h = make_stages(2, &[
            ("time",  0, ParameterValue::Float(0.0)),
            ("level", 0, ParameterValue::Float(0.8)),
            ("time",  1, ParameterValue::Float(0.5)),
            ("level", 1, ParameterValue::Float(0.0)),
        ]);
        h.set_poly("trigger", arr(1.0, 0));
        h.set_poly("gate", arr(1.0, 0));
        h.tick();
        assert_within!(0.8, h.read_poly_voice("out", 0), 1e-6_f32);
        // Hold under gate.
        h.set_poly("trigger", [0.0; 16]);
        for _ in 0..5 {
            h.tick();
        }
        assert_within!(0.8, h.read_poly_voice("out", 0), 1e-6_f32);
        // Gate off: release ramps 0.8 → 0 over 5 samples.
        h.set_poly("gate", [0.0; 16]);
        let expected = [0.64, 0.48, 0.32, 0.16, 0.0];
        for &exp in &expected {
            h.tick();
            assert_within!(exp, h.read_poly_voice("out", 0), 1e-5_f32);
        }
    }
}
