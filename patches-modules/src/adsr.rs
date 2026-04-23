use patches_core::{
    params_enum,
    AudioEnvironment, CablePool, GateInput, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoOutput, ModuleShape, OutputPort,
};
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
        attack:  Float,
        decay:   Float,
        sustain: Float,
        release: Float,
        shape:   Enum<AdsrShapeParam>,
    }
}


/// An ADSR envelope generator.
///
/// Trigger rising edge starts the Attack phase. Gate held high sustains at the
/// sustain level; releasing gate transitions to Release. Output is always in [0.0, 1.0].
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `trigger` | trigger | One-sample pulse starts Attack phase (ADR 0047) |
/// | `gate` | mono | Held high to sustain; release to enter Release phase |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | mono | Envelope level in [0.0, 1.0] |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `attack` | float | 0.001 -- 10.0 | `0.01` | Attack time in seconds |
/// | `decay` | float | 0.001 -- 10.0 | `0.1` | Decay time in seconds |
/// | `sustain` | float | 0.0 -- 1.0 | `0.7` | Sustain level |
/// | `release` | float | 0.001 -- 10.0 | `0.3` | Release time in seconds |
/// | `shape` | enum | linear, exponential | `linear` | Segment shape: linear ramp (default) or analog-style RC curve |
pub struct Adsr {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    // Parameters (stored for re-application when sample_rate is known)
    attack_secs: f32,
    decay_secs: f32,
    sustain: f32,
    release_secs: f32,
    // Core DSP state
    core: AdsrCore,
    // Port fields
    in_trigger: TriggerInput,
    in_gate: GateInput,
    out_env: MonoOutput,
}

impl Module for Adsr {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Adsr", shape.clone())
            .trigger_in("trigger")
            .mono_in("gate")
            .mono_out("out")
            .float_param(params::attack,  0.001, 10.0, 0.01)
            .float_param(params::decay,   0.001, 10.0, 0.1)
            .float_param(params::sustain, 0.0,   1.0,  0.7)
            .float_param(params::release, 0.001, 10.0, 0.3)
            .enum_param(params::shape, AdsrShapeParam::Linear)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            attack_secs: 0.0,
            decay_secs: 0.0,
            sustain: 0.0,
            release_secs: 0.0,
            core: AdsrCore::new(audio_environment.sample_rate),
            in_trigger: TriggerInput::default(),
            in_gate: GateInput::default(),
            out_env: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.attack_secs = p.get(params::attack);
        self.decay_secs = p.get(params::decay);
        self.sustain = p.get(params::sustain);
        self.release_secs = p.get(params::release);
        self.core.set_params(self.attack_secs, self.decay_secs, self.sustain, self.release_secs);
        let shape: AdsrShapeParam = p.get(params::shape);
        self.core.set_shape(shape.into());
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_trigger = TriggerInput::from_ports(inputs, 0);
        self.in_gate = GateInput::from_ports(inputs, 1);
        self.out_env = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let triggered = self.in_trigger.tick(pool).is_some();
        let gate = self.in_gate.tick(pool);
        let level = self.core.tick(triggered, gate.is_high);
        pool.write_mono(&self.out_env, level);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::AudioEnvironment;
    use patches_core::test_support::{assert_within, ModuleHarness, params};

    fn make_envelope(attack: f32, decay: f32, sustain: f32, release: f32) -> ModuleHarness {
        ModuleHarness::build_with_env::<Adsr>(
            params!["attack" => attack, "decay" => decay, "sustain" => sustain, "release" => release],
            AudioEnvironment { sample_rate: 10.0, poly_voices: 16, periodic_update_interval: 32, hosted: false },
        )
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
    fn sustain_holds_while_gate_high() {
        let mut h = make_envelope(0.1, 0.1, 0.6, 1.0);
        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        h.tick(); // decay completes in 1 sample

        for _ in 0..5 {
            h.tick();
            let v = h.read_mono("out");
            assert_within!(0.6, v, 1e-12_f32);
        }
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

}
