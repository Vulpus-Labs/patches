use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, OutputPort, PolyInput, PolyOutput,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_dsp::AdsrCore;

/// Polyphonic ADSR envelope generator.
///
/// Maintains one envelope state machine per voice. Shared ADSR parameters apply to all
/// voices. Each voice is driven by its own trigger/gate channel from the poly inputs.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `trigger` | poly | Rising edge starts Attack phase per voice |
/// | `gate` | poly | Held high to sustain; release to enter Release phase per voice |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | poly | Envelope level in [0.0, 1.0] per voice |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `attack` | float | 0.001 -- 10.0 | `0.01` | Attack time in seconds |
/// | `decay` | float | 0.001 -- 10.0 | `0.1` | Decay time in seconds |
/// | `sustain` | float | 0.0 -- 1.0 | `0.7` | Sustain level |
/// | `release` | float | 0.001 -- 10.0 | `0.3` | Release time in seconds |
pub struct PolyAdsr {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    voices: [AdsrCore; 16],
    // Shared parameters (stored for re-application)
    attack_secs: f32,
    decay_secs: f32,
    sustain: f32,
    release_secs: f32,
    // Port fields
    in_trigger: PolyInput,
    in_gate: PolyInput,
    out_env: PolyOutput,
}

impl Module for PolyAdsr {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("PolyAdsr", shape.clone())
            .poly_in("trigger")
            .poly_in("gate")
            .poly_out("out")
            .float_param("attack",  0.001, 10.0, 0.01)
            .float_param("decay",   0.001, 10.0, 0.1)
            .float_param("sustain", 0.0,   1.0,  0.7)
            .float_param("release", 0.001, 10.0, 0.3)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let sr = audio_environment.sample_rate;
        Self {
            instance_id,
            descriptor,
            voices: std::array::from_fn(|_| AdsrCore::new(sr)),
            attack_secs: 0.0,
            decay_secs: 0.0,
            sustain: 0.0,
            release_secs: 0.0,
            in_trigger: PolyInput::default(),
            in_gate: PolyInput::default(),
            out_env: PolyOutput::default(),
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
            for voice in &mut self.voices {
                voice.set_params(self.attack_secs, self.decay_secs, self.sustain, self.release_secs);
            }
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_trigger = PolyInput::from_ports(inputs, 0);
        self.in_gate    = PolyInput::from_ports(inputs, 1);
        self.out_env    = PolyOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let trigger_arr = pool.read_poly(&self.in_trigger);
        let gate_arr    = pool.read_poly(&self.in_gate);

        let mut out = [0.0f32; 16];

        for i in 0..16 {
            out[i] = self.voices[i].tick(trigger_arr[i], gate_arr[i]);
        }

        pool.write_poly(&self.out_env, out);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::AudioEnvironment;
    use patches_core::test_support::{assert_within, ModuleHarness, params};

    fn make_adsr(attack: f32, decay: f32, sustain: f32, release: f32, voices: usize) -> ModuleHarness {
        ModuleHarness::build_with_env::<PolyAdsr>(
            params!["attack" => attack, "decay" => decay, "sustain" => sustain, "release" => release],
            AudioEnvironment { sample_rate: 10.0, poly_voices: voices, periodic_update_interval: 32 },
        )
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
        assert_eq!(out[1], 0.0); // voice 1 not triggered
    }

    #[test]
    fn sustain_holds_while_gate_high() {
        // attack=0.1s → 1 sample at 10 Hz, inc=1.0; decay=0.1s → 1 sample; sustain=0.6
        let mut h = make_adsr(0.1, 0.1, 0.6, 1.0, 2);
        h.set_poly("trigger", arr(1.0, 0));
        h.set_poly("gate",    arr(1.0, 0));
        h.tick(); // voice 0: attack → level=1.0
        h.set_poly("trigger", arr(0.0, 0));
        h.tick(); // voice 0: decay → level=0.6

        for _ in 0..5 {
            h.tick();
            // exact arithmetic at sample_rate=10; 1e-12 accounts for f32 rounding only
            assert_within!(0.6, h.read_poly_voice("out", 0), 1e-12_f32);
            assert_eq!(h.read_poly_voice("out", 1), 0.0, "voice 1 must remain silent");
        }
    }

    #[test]
    fn release_falls_to_zero() {
        // attack=0.1s → 1 sample; decay=0.1s → 1 sample; sustain=0.5; release=0.5s → 5 samples
        // release_inc = 0.5 / (0.5 * 10) = 0.1 per sample
        let mut h = make_adsr(0.1, 0.1, 0.5, 0.5, 2);
        h.set_poly("trigger", arr(1.0, 0));
        h.set_poly("gate",    arr(1.0, 0));
        h.tick(); // attack
        h.set_poly("trigger", arr(0.0, 0));
        h.tick(); // decay → sustain

        h.set_poly("gate", arr(0.0, 0));
        // exact arithmetic at sample_rate=10; 1e-5 accounts for accumulated f32 rounding
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
        h.tick(); // attack
        h.set_poly("trigger", arr(0.0, 0));
        h.tick(); // decay → sustain
        h.set_poly("gate", arr(0.0, 0));
        h.tick();
        h.tick(); // two steps into release

        // Retrigger voice 0 mid-release; attack inc=1.0/sample → reaches 1.0 immediately
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
        // attack=0.1s (1 sample), decay=0.5s (5 samples), sustain=0.5
        let mut h = make_adsr(0.1, 0.5, 0.5, 0.5, 2);

        // Trigger voice 0
        h.set_poly("trigger", arr(1.0, 0));
        h.set_poly("gate",    arr(1.0, 0));
        h.tick();

        // Voice 0 in Decay; trigger voice 1
        h.set_poly("trigger", arr(1.0, 1));
        h.set_poly("gate",    arr(1.0, 1));
        h.tick();
        let out = h.read_poly("out");
        // Voice 0 decaying (1.0 - 0.1 = 0.9), voice 1 just started attack (1.0)
        assert_within!(0.9, out[0], 1e-10_f32);
        assert_within!(1.0, out[1], 1e-10_f32);
    }
}
