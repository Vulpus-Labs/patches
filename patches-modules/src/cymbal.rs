/// Cymbal synthesiser (crash/ride).
///
/// Uses the same metallic tone engine as hi-hats but with a higher frequency
/// range, longer decay, and a "shimmer" parameter that adds slow LFO
/// modulation to the partial frequencies.
///
/// # Inputs
///
/// | Port       | Kind | Description                                                                                      |
/// |------------|------|--------------------------------------------------------------------------------------------------|
/// | `trigger`  | mono | Rising edge triggers                                                                             |
/// | `velocity` | mono | Velocity (0.0–1.0); latched on trigger, scales output amplitude. Defaults to 1.0 when disconnected |
///
/// # Outputs
///
/// | Port  | Kind | Description   |
/// |-------|------|---------------|
/// | `out` | mono | Cymbal signal |
///
/// # Parameters
///
/// | Name      | Type  | Range         | Default | Description                        |
/// |-----------|-------|---------------|---------|------------------------------------|
/// | `pitch`   | float | 200–10000 Hz  | 600     | Base frequency of metallic tone    |
/// | `decay`   | float | 0.2–8.0 s     | 2.0     | Amplitude decay time               |
/// | `tone`    | float | 0.0–1.0       | 0.5     | Metallic vs noise mix              |
/// | `filter`  | float | 2000–16000 Hz | 6000    | Noise highpass cutoff              |
/// | `shimmer` | float | 0.0–1.0       | 0.2     | Partial frequency modulation depth |
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoInput, MonoOutput, OutputPort, TriggerInput,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_dsp::drum::{DecayEnvelope, MetallicTone};
use patches_dsp::{SvfKernel, svf_f, q_to_damp, xorshift64};

pub struct Cymbal {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    pitch: f32,
    decay_time: f32,
    tone: f32,
    filter_freq: f32,
    shimmer: f32,
    mod_depth: f32,
    latched_velocity: f32,
    metallic: MetallicTone,
    amp_env: DecayEnvelope,
    hp_filter: SvfKernel,
    prng_state: u64,
    lfo_phase: f32,
    lfo_increment: f32,
    in_trigger: TriggerInput,
    in_velocity: MonoInput,
    out_audio: MonoOutput,
}

impl Module for Cymbal {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Cymbal", shape.clone())
            .mono_in("trigger")
            .mono_in("velocity")
            .mono_out("out")
            .float_param("pitch", 200.0, 10000.0, 600.0)
            .float_param("decay", 0.2, 8.0, 2.0)
            .float_param("tone", 0.0, 1.0, 0.5)
            .float_param("filter", 2000.0, 16000.0, 6000.0)
            .float_param("shimmer", 0.0, 1.0, 0.2)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let sr = audio_environment.sample_rate;
        let mut amp_env = DecayEnvelope::new(sr);
        amp_env.set_decay(2.0);
        let mut metallic = MetallicTone::new(sr);
        metallic.set_frequency(600.0);
        let f = svf_f(6000.0, sr);
        let d = q_to_damp(0.3);
        // Shimmer LFO at ~3 Hz
        let lfo_increment = 3.0 / sr;
        Self {
            instance_id,
            descriptor,
            sample_rate: sr,
            pitch: 600.0,
            decay_time: 2.0,
            tone: 0.5,
            filter_freq: 6000.0,
            shimmer: 0.2,
            mod_depth: 0.2 * 20.0,
            latched_velocity: 1.0,
            metallic,
            amp_env,
            hp_filter: SvfKernel::new_static(f, d),
            prng_state: instance_id.as_u64() + 1,
            lfo_phase: 0.0,
            lfo_increment,
            in_trigger: TriggerInput::default(),
            in_velocity: MonoInput::default(),
            out_audio: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("pitch") {
            self.pitch = *v;
            self.metallic.set_frequency(self.pitch);
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("decay") {
            self.decay_time = *v;
            self.amp_env.set_decay(self.decay_time);
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("tone") {
            self.tone = *v;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("filter") {
            self.filter_freq = *v;
            let f = svf_f(self.filter_freq, self.sample_rate);
            let d = q_to_damp(0.3);
            self.hp_filter = SvfKernel::new_static(f, d);
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("shimmer") {
            self.shimmer = *v;
            self.mod_depth = self.shimmer * 20.0;
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_trigger = TriggerInput::from_ports(inputs, 0);
        self.in_velocity = MonoInput::from_ports(inputs, 1);
        self.out_audio = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let trigger_rose = self.in_trigger.tick(pool);

        if trigger_rose {
            self.latched_velocity = if self.in_velocity.connected {
                pool.read_mono(&self.in_velocity)
            } else {
                1.0
            };
            self.metallic.trigger();
            self.lfo_phase = 0.0;
        }

        let amp = self.amp_env.tick(trigger_rose);

        // Metallic tone with shimmer modulation
        let metal = self.metallic.tick_with_modulation(self.mod_depth, self.lfo_phase);

        // Advance LFO
        self.lfo_phase += self.lfo_increment;
        if self.lfo_phase >= 1.0 {
            self.lfo_phase -= 1.0;
        }

        // Highpass-filtered noise
        let white = xorshift64(&mut self.prng_state);
        let (_lp, hp, _bp) = self.hp_filter.tick(white);

        let mix = metal * self.tone + hp * (1.0 - self.tone);
        let output = mix * amp;

        pool.write_mono(&self.out_audio, output * self.latched_velocity);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;

    #[test]
    fn trigger_produces_output() {
        let mut h = ModuleHarness::build::<Cymbal>(&[]);
        h.disconnect_input("velocity");
        h.set_mono("trigger", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        let rms = h.measure_rms(5000, "out");
        assert!(rms > 0.001, "cymbal should produce output, rms = {rms}");
    }

    #[test]
    fn long_decay() {
        let mut h = ModuleHarness::build::<Cymbal>(&[
            ("decay", ParameterValue::Float(4.0)),
        ]);
        h.disconnect_input("velocity");
        h.set_mono("trigger", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);

        // At 1s in, should still be audible
        for _ in 0..44100 {
            h.tick();
        }
        let rms = h.measure_rms(1000, "out");
        assert!(rms > 0.001, "cymbal with 4s decay should still ring at 1s, rms = {rms}");
    }

    #[test]
    fn shimmer_produces_modulation() {
        // With shimmer=0 and shimmer=1, output should differ
        let mut h_no = ModuleHarness::build::<Cymbal>(&[
            ("shimmer", ParameterValue::Float(0.0)),
            ("tone", ParameterValue::Float(1.0)), // all metallic
        ]);
        h_no.disconnect_input("velocity");
        let mut h_yes = ModuleHarness::build::<Cymbal>(&[
            ("shimmer", ParameterValue::Float(1.0)),
            ("tone", ParameterValue::Float(1.0)),
        ]);
        h_yes.disconnect_input("velocity");

        h_no.set_mono("trigger", 1.0);
        h_no.tick();
        h_no.set_mono("trigger", 0.0);
        h_yes.set_mono("trigger", 1.0);
        h_yes.tick();
        h_yes.set_mono("trigger", 0.0);

        let s_no = h_no.run_mono(2000, "out");
        let s_yes = h_yes.run_mono(2000, "out");

        // They should differ (shimmer modulates frequencies)
        let diff: f32 = s_no.iter().zip(s_yes.iter()).map(|(a, b)| (a - b).abs()).sum::<f32>() / 2000.0;
        assert!(diff > 0.001, "shimmer should change the output, avg diff = {diff}");
    }
}
