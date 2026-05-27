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
use patches_dsp::{AdsrShape, EnvCore, Stage, MAX_STAGES};

params_enum! {
    pub enum EnvCurveParam {
        Linear => "linear",
        Exponential => "exponential",
    }
}

impl From<EnvCurveParam> for AdsrShape {
    fn from(p: EnvCurveParam) -> Self {
        match p {
            EnvCurveParam::Linear => AdsrShape::Linear,
            EnvCurveParam::Exponential => AdsrShape::Exponential,
        }
    }
}

module_params! {
    Env {
        time:      FloatArray,
        level:     FloatArray,
        curve:     EnumArray<EnvCurveParam>,
        keyfollow: Float,
        ref_key:   Float,
        vel_depth: Float,
    }
}

/// Multi-stage breakpoint envelope with key-follow time-scaling, velocity
/// scaling, and a built-in VCA pass-through.
///
/// Where `Adsr` has a fixed attack/decay/sustain/release shape, `Env` runs an
/// arbitrary number of `(time, level, curve)` stages with a designated sustain
/// stage — enough to express D50-style contours ADSR cannot (attack spike → dip
/// → secondary swell → sustain). The stage count is the module's *channels*
/// axis: `Env(5)` is a five-stage envelope. One module is one envelope (mono);
/// use multiple instances for multiple envelopes.
///
/// Stages `0..sustain_stage` form the pre-sustain contour; the envelope holds
/// at `level[sustain_stage]` while the gate is high, then runs the remaining
/// stages `sustain_stage+1..N` as the release tail on gate-off. Designate a
/// final stage with `level = 0` for a release to silence. A re-trigger restarts
/// from the current level (no click), and a gate-off during the pre-sustain
/// contour jumps straight into the release tail.
///
/// **Key-follow** shortens stage times with pitch, as real resonators decay
/// faster up the keyboard: `time_scale = 2^(-keyfollow * (voct - ref_key))`, so
/// `keyfollow = 1.0` halves all stage times one octave above `ref_key`. It is
/// applied per tick, so a bending `voct` re-scales the contour live.
///
/// **Velocity** scales stage *levels*: the effective `velocity` (1.0 if the
/// input is unconnected) is mapped to a level multiplier
/// `1 - vel_depth * (1 - velocity)` and latched at the trigger, so an in-flight
/// envelope keeps a stable scaling.
///
/// Route `out` to an oscillator `voct`/`phase_mod` or a filter cutoff for the
/// D50 attack pitch-blip or filter sweep — that's a patch idiom, not a port.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `trigger` | trigger | One-sample pulse starts the envelope (ADR 0047) |
/// | `gate` | mono | Held high to sustain; release to run the release tail |
/// | `voct` | mono | 1V/oct pitch driving key-follow time-scaling (0 if unconnected) |
/// | `velocity` | mono | Velocity in \[0, 1\] scaling stage levels (1.0 if unconnected) |
/// | `vca_in` | mono | Optional audio/CV input multiplied by the envelope |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | mono | Envelope level in \[0.0, 1.0\] |
/// | `vca_out` | mono | `vca_in * out` — pre-multiplied audio/CV |
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
pub struct Env {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    /// Number of stages = the channels axis, capped at [`MAX_STAGES`].
    stages: usize,
    /// Structural: index of the held sustain stage.
    sustain_stage: usize,
    core: EnvCore,
    keyfollow: f32,
    ref_key: f32,
    vel_depth: f32,
    // Port fields
    in_trigger: TriggerInput,
    in_gate: GateInput,
    in_voct: MonoInput,
    in_velocity: MonoInput,
    in_vca: MonoInput,
    out_env: MonoOutput,
    out_vca: MonoOutput,
}

impl Module for Env {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "Env",
            // The channels axis is the stage count for this module.
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[
                PortTemplate::trigger("trigger"),
                PortTemplate::mono("gate"),
                PortTemplate::mono("voct"),
                PortTemplate::mono("velocity"),
                PortTemplate::mono("vca_in"),
            ],
            per_axis_inputs: &[],
            global_outputs: &[
                PortTemplate::mono("out"),
                PortTemplate::mono("vca_out"),
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
                        variants: EnvCurveParam::VARIANTS,
                        default: "linear",
                    },
                }),
            ],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, structural: &StructuralParams) -> Result<Self, BuildError> { Ok({
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
            core: EnvCore::new(audio_environment.sample_rate),
            keyfollow: 0.0,
            ref_key: 0.0,
            vel_depth: 0.0,
            in_trigger: TriggerInput::default(),
            in_gate: GateInput::default(),
            in_voct: MonoInput::default(),
            in_velocity: MonoInput::default(),
            in_vca: MonoInput::default(),
            out_env: MonoOutput::default(),
            out_vca: MonoOutput::default(),
        }
    })}

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.keyfollow = p.get(params::keyfollow);
        self.ref_key = p.get(params::ref_key);
        self.vel_depth = p.get(params::vel_depth);

        // Build the stage list on the stack (no allocation) and hand it to the
        // core, then re-apply the sustain index (set_stages re-clamps it).
        let mut stages = [Stage::default(); MAX_STAGES];
        for c in 0..self.stages {
            let idx = c as u16;
            stages[c] = Stage::new(
                p.get(params::level.at(idx)),
                p.get(params::time.at(idx)),
                p.get(params::curve.at(idx)).into(),
            );
        }
        self.core.set_stages(&stages[..self.stages]);
        self.core.set_sustain_stage(self.sustain_stage);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        // Input order: trigger(0), gate(1), voct(2), velocity(3), vca_in(4).
        self.in_trigger = TriggerInput::from_ports(inputs, 0);
        self.in_gate = GateInput::from_ports(inputs, 1);
        self.in_voct = MonoInput::from_ports(inputs, 2);
        self.in_velocity = MonoInput::from_ports(inputs, 3);
        self.in_vca = MonoInput::from_ports(inputs, 4);
        // Output order: out(0), vca_out(1).
        self.out_env = MonoOutput::from_ports(outputs, 0);
        self.out_vca = MonoOutput::from_ports(outputs, 1);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let triggered = self.in_trigger.tick(pool).is_some();
        let gate_high = self.in_gate.tick(pool).is_high;

        // Velocity → level scale, latched at trigger inside the core.
        let velocity = if self.in_velocity.is_connected() {
            pool.read_mono(&self.in_velocity).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.core.set_level_scale(1.0 - self.vel_depth * (1.0 - velocity));

        // Key-follow: 2^(-keyfollow * (voct - ref_key)), applied per tick.
        let voct = if self.in_voct.is_connected() {
            pool.read_mono(&self.in_voct)
        } else {
            0.0
        };
        let time_scale = (-self.keyfollow * (voct - self.ref_key)).exp2();

        let level = self.core.tick(triggered, gate_high, time_scale);

        if self.out_env.is_connected() {
            pool.write_mono(&self.out_env, level);
        }
        if self.out_vca.is_connected() {
            let sig = pool.read_mono(&self.in_vca);
            pool.write_mono(&self.out_vca, sig * level);
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

    /// Single-stage envelope (sustain at stage 0) with the given parameters.
    fn make_single(entries: &[(&str, ParameterValue)]) -> ModuleHarness {
        ModuleHarness::build_full::<Env>(entries, env_at_10hz(), ModuleShape { channels: 1 })
    }

    /// Build an `n`-stage envelope; per-stage params as `(name, stage, value)`.
    fn make_stages(n: usize, entries: &[(&str, usize, ParameterValue)]) -> ModuleHarness {
        let mut map = ParameterMap::new();
        for (name, idx, val) in entries {
            map.insert_param(name.to_string(), *idx, val.clone());
        }
        let mut h = ModuleHarness::build_full::<Env>(&[], env_at_10hz(), ModuleShape { channels: n });
        h.update_params_map(&map);
        h
    }

    /// Key-follow halves stage times one octave up: at `keyfollow = 1.0` and
    /// `voct = 1.0`, the per-tick rate doubles versus `voct = 0.0`.
    #[test]
    fn keyfollow_halves_time_one_octave_up() {
        let first_step = |voct: f32| -> f32 {
            // 1 stage, time 0.4s (4 samples at 10 Hz), ramp to 1.0, keyfollow 1.0.
            let mut h = make_single(params![
                "time" => 0.4_f32, "level" => 1.0_f32, "keyfollow" => 1.0_f32
            ]);
            h.set_mono("voct", voct);
            h.set_mono("trigger", 1.0);
            h.set_mono("gate", 1.0);
            h.tick();
            h.read_mono("out")
        };
        let base = first_step(0.0); // inc = 0.25
        let octave_up = first_step(1.0); // time halved → inc = 0.5
        assert_within!(0.25, base, 1e-6_f32);
        assert_within!(0.5, octave_up, 1e-6_f32);
    }

    /// Velocity attenuates stage levels: with `vel_depth = 1.0`, a velocity of
    /// 0.5 holds the sustain at half its nominal level.
    #[test]
    fn velocity_scales_level() {
        // 1 stage, instant (time 0) to level 1.0, sustain at stage 0.
        let mut h = make_single(params![
            "time" => 0.0_f32, "level" => 1.0_f32, "vel_depth" => 1.0_f32
        ]);
        h.set_mono("velocity", 0.5);
        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        assert_within!(0.5, h.read_mono("out"), 1e-6_f32);
        // Held while gate high.
        h.set_mono("trigger", 0.0);
        h.tick();
        assert_within!(0.5, h.read_mono("out"), 1e-6_f32);
    }

    /// Unconnected velocity behaves as full velocity (no attenuation) even with
    /// `vel_depth = 1.0`.
    #[test]
    fn unconnected_velocity_is_full() {
        let mut h = make_single(params![
            "time" => 0.0_f32, "level" => 1.0_f32, "vel_depth" => 1.0_f32
        ]);
        h.disconnect_input("velocity");
        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        assert_within!(1.0, h.read_mono("out"), 1e-6_f32);
    }

    /// VCA pass-through equals `vca_in * env`.
    #[test]
    fn vca_out_equals_input_times_env() {
        let mut h = make_single(params!["time" => 0.0_f32, "level" => 1.0_f32]);
        h.set_mono("vca_in", 0.6);
        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        // Instant stage → env = 1.0, so vca_out = vca_in.
        assert_within!(1.0, h.read_mono("out"), 1e-6_f32);
        assert_within!(0.6, h.read_mono("vca_out"), 1e-6_f32);
    }

    /// Two-stage envelope (sustain stage 0, release stage 1) holds at the
    /// sustain level under gate, then runs the release tail to zero.
    #[test]
    fn sustain_then_release_tail() {
        // Stage 0 (sustain): instant to 0.8. Stage 1 (release): ramp to 0 over
        // 0.5s = 5 samples.
        let mut h = make_stages(2, &[
            ("time",  0, ParameterValue::Float(0.0)),
            ("level", 0, ParameterValue::Float(0.8)),
            ("time",  1, ParameterValue::Float(0.5)),
            ("level", 1, ParameterValue::Float(0.0)),
        ]);
        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        assert_within!(0.8, h.read_mono("out"), 1e-6_f32);
        // Hold under gate.
        h.set_mono("trigger", 0.0);
        for _ in 0..5 {
            h.tick();
        }
        assert_within!(0.8, h.read_mono("out"), 1e-6_f32);
        // Gate off: release ramps 0.8 → 0 over 5 samples.
        h.set_mono("gate", 0.0);
        let expected = [0.64, 0.48, 0.32, 0.16, 0.0];
        for &exp in &expected {
            h.tick();
            assert_within!(exp, h.read_mono("out"), 1e-5_f32);
        }
        h.tick();
        assert_eq!(h.read_mono("out"), 0.0, "idle after release");
    }

    #[test]
    fn idle_output_is_zero() {
        let mut h = make_single(params!["time" => 0.5_f32, "level" => 1.0_f32]);
        h.set_mono("trigger", 0.0);
        h.set_mono("gate", 0.0);
        h.tick();
        assert_eq!(h.read_mono("out"), 0.0);
        h.tick();
        assert_eq!(h.read_mono("out"), 0.0);
    }
}
