use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_dsp::AdsrCore;

/// An ADSR envelope generator.
///
/// Input ports:
///   inputs[0] — trigger (rising edge starts Attack)
///   inputs[1] — gate    (held high keeps Sustain; releasing transitions to Release)
///
/// Output ports:
///   outputs[0] — out (envelope level, always in [0.0, 1.0])
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
    in_trigger: MonoInput,
    in_gate: MonoInput,
    out_env: MonoOutput,
}

impl Module for Adsr {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Adsr", shape.clone())
            .mono_in("trigger")
            .mono_in("gate")
            .mono_out("out")
            .float_param("attack",  0.001, 10.0, 0.01)
            .float_param("decay",   0.001, 10.0, 0.1)
            .float_param("sustain", 0.0,   1.0,  0.7)
            .float_param("release", 0.001, 10.0, 0.3)
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
            in_trigger: MonoInput::default(),
            in_gate: MonoInput::default(),
            out_env: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        let mut changed = false;
        if let Some(ParameterValue::Float(v)) = params.get_scalar("attack") {
            self.attack_secs = *v;
            changed = true;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("decay") {
            self.decay_secs = *v;
            changed = true;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("sustain") {
            self.sustain = *v;
            changed = true;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("release") {
            self.release_secs = *v;
            changed = true;
        }
        if changed {
            self.core.set_params(self.attack_secs, self.decay_secs, self.sustain, self.release_secs);
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_trigger = MonoInput::from_ports(inputs, 0);
        self.in_gate = MonoInput::from_ports(inputs, 1);
        self.out_env = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let trigger = pool.read_mono(&self.in_trigger);
        let gate = pool.read_mono(&self.in_gate);
        let level = self.core.tick(trigger, gate);
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
            AudioEnvironment { sample_rate: 10.0, poly_voices: 16, periodic_update_interval: 32 },
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
