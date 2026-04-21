/// Transient shaper using dual envelope followers.
///
/// Independently boosts or cuts attack transients and sustained tails using
/// a fast and a slow envelope follower. Essential for adding punch or taming
/// dynamics on drum hits.
///
/// The fast follower tracks transients closely; the slow follower (4x the
/// speed time constant) tracks the sustained level. The difference between
/// them isolates the transient component.
///
/// # Inputs
///
/// | Port  | Kind | Description  |
/// |-------|------|--------------|
/// | `in`  | mono | Audio input  |
///
/// # Outputs
///
/// | Port  | Kind | Description      |
/// |-------|------|------------------|
/// | `out` | mono | Processed output |
///
/// # Parameters
///
/// | Name      | Type  | Range        | Default | Description                            |
/// |-----------|-------|--------------|---------|----------------------------------------|
/// | `attack`  | float | -1.0--1.0    | `0.0`   | Boost (+) or cut (-) transient attack  |
/// | `sustain` | float | -1.0--1.0    | `0.0`   | Boost (+) or cut (-) sustained portion |
/// | `speed`   | float | 1.0--100.0   | `20.0`  | Detector speed in ms                   |
/// | `mix`     | float | 0.0--1.0     | `1.0`   | Dry/wet blend                          |
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::param_frame::ParamView;
use patches_dsp::EnvelopeFollower;

pub struct TransientShaper {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    attack_amount: f32,
    sustain_amount: f32,
    speed_ms: f32,
    mix: f32,
    fast_env: EnvelopeFollower,
    slow_env: EnvelopeFollower,
    in_audio: MonoInput,
    out_audio: MonoOutput,
}

impl TransientShaper {
    fn configure_envelopes(&mut self) {
        let sr = self.sample_rate;
        // Fast envelope: attack = speed, release = speed * 2
        self.fast_env.set_attack_ms(self.speed_ms, sr);
        self.fast_env.set_release_ms(self.speed_ms * 2.0, sr);
        // Slow envelope: attack = speed * 4, release = speed * 4
        self.slow_env.set_attack_ms(self.speed_ms * 4.0, sr);
        self.slow_env.set_release_ms(self.speed_ms * 4.0, sr);
    }
}

impl Module for TransientShaper {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("TransientShaper", shape.clone())
            .mono_in("in")
            .mono_out("out")
            .float_param("attack", -1.0, 1.0, 0.0)
            .float_param("sustain", -1.0, 1.0, 0.0)
            .float_param("speed", 1.0, 100.0, 20.0)
            .float_param("mix", 0.0, 1.0, 1.0)
    }

    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let mut s = Self {
            instance_id,
            descriptor,
            sample_rate: env.sample_rate,
            attack_amount: 0.0,
            sustain_amount: 0.0,
            speed_ms: 20.0,
            mix: 1.0,
            fast_env: EnvelopeFollower::new(),
            slow_env: EnvelopeFollower::new(),
            in_audio: MonoInput::default(),
            out_audio: MonoOutput::default(),
        };
        s.configure_envelopes();
        s
    }

    fn update_validated_parameters(&mut self, params: &ParamView<'_>) {
        let mut speed_changed = false;
        let v = params.float("attack");
        self.attack_amount = v.clamp(-1.0, 1.0);
        let v = params.float("sustain");
        self.sustain_amount = v.clamp(-1.0, 1.0);
        let v = params.float("speed");
        self.speed_ms = v.clamp(1.0, 100.0);
        speed_changed = true;
        let v = params.float("mix");
        self.mix = v.clamp(0.0, 1.0);
        if speed_changed {
            self.configure_envelopes();
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_audio = MonoInput::from_ports(inputs, 0);
        self.out_audio = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let dry = pool.read_mono(&self.in_audio);

        let fast = self.fast_env.tick(dry);
        let slow = self.slow_env.tick(dry);

        // Transient component: positive during attacks
        let transient = (fast - slow).max(0.0);
        // Sustain component: the slow envelope
        let sustain = slow;

        // Gain envelope
        let gain = 1.0 + self.attack_amount * transient + self.sustain_amount * sustain;

        let wet = dry * gain;
        let out = self.mix * wet + (1.0 - self.mix) * dry;
        pool.write_mono(&self.out_audio, out);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use patches_core::ParameterValue;
    use super::*;
    use patches_core::test_support::{assert_nearly, ModuleHarness, params};

    #[test]
    fn descriptor_shape() {
        let h = ModuleHarness::build::<TransientShaper>(&[]);
        let desc = h.descriptor();
        assert_eq!(desc.inputs.len(), 1);
        assert_eq!(desc.outputs.len(), 1);
        assert_eq!(desc.inputs[0].name, "in");
        assert_eq!(desc.outputs[0].name, "out");
    }

    #[test]
    fn zero_attack_zero_sustain_passes_through() {
        let mut h = ModuleHarness::build::<TransientShaper>(
            params!["attack" => 0.0_f32, "sustain" => 0.0_f32, "speed" => 20.0_f32, "mix" => 1.0_f32],
        );
        // With attack=0, sustain=0, gain = 1.0 always → output = input
        h.set_mono("in", 0.5);
        h.tick();
        assert_nearly!(0.5, h.read_mono("out"));
    }

    #[test]
    fn positive_attack_boosts_transient() {
        let mut h = ModuleHarness::build::<TransientShaper>(
            params!["attack" => 1.0_f32, "sustain" => 0.0_f32, "speed" => 5.0_f32, "mix" => 1.0_f32],
        );
        // Feed silence then a sudden burst
        for _ in 0..441 {
            h.set_mono("in", 0.0);
            h.tick();
        }
        // Transient: fast env rises, slow env lags → transient > 0 → gain > 1
        h.set_mono("in", 0.5);
        // Run a few samples so the fast env reacts
        for _ in 0..22 {
            h.tick();
        }
        let out = h.read_mono("out");
        // With attack boost, output should exceed the input
        assert!(out.abs() > 0.5, "positive attack should boost transient, got {out}");
    }

    #[test]
    fn mix_zero_passes_dry() {
        let mut h = ModuleHarness::build::<TransientShaper>(
            params!["attack" => 1.0_f32, "sustain" => -1.0_f32, "mix" => 0.0_f32],
        );
        h.set_mono("in", 0.42);
        h.tick();
        assert_nearly!(0.42, h.read_mono("out"));
    }

    #[test]
    fn output_stays_bounded_under_extreme_settings() {
        let mut h = ModuleHarness::build::<TransientShaper>(
            params!["attack" => 1.0_f32, "sustain" => 1.0_f32, "speed" => 1.0_f32, "mix" => 1.0_f32],
        );
        // Bound: the shaper is a gain modulator on |input|; with input ∈ {0, 1}
        // and full attack+sustain boost, output magnitude should stay within a
        // small multiple of the peak input. A loose finite-only check would
        // accept ±1e30; assert a real envelope here.
        const MAX_GAIN: f32 = 8.0;
        let mut max_seen = 0.0_f32;
        for i in 0..1000 {
            let x = if i < 10 { 1.0 } else { 0.0 };
            h.set_mono("in", x);
            h.tick();
            let out = h.read_mono("out");
            assert!(out.is_finite(), "non-finite output at sample {i}");
            assert!(
                out.abs() <= MAX_GAIN,
                "output {out} at sample {i} exceeds bound {MAX_GAIN}"
            );
            max_seen = max_seen.max(out.abs());
        }
        // Sanity: with attack=1.0 the burst must produce some boost above input.
        assert!(max_seen > 1.0, "expected attack boost, peak was {max_seen}");
    }
}
