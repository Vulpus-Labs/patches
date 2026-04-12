/// 808-style tom synthesiser.
///
/// Shares the kick's basic architecture (sine oscillator + pitch sweep +
/// amplitude decay) but with a higher pitch range, shorter sweep, and a
/// subtle noise layer for attack texture.
///
/// # Inputs
///
/// | Port      | Kind | Description          |
/// |-----------|------|----------------------|
/// | `trigger` | mono | Rising edge triggers |
///
/// # Outputs
///
/// | Port  | Kind | Description |
/// |-------|------|-------------|
/// | `out` | mono | Tom signal  |
///
/// # Parameters
///
/// | Name         | Type  | Range       | Default | Description              |
/// |--------------|-------|-------------|---------|--------------------------|
/// | `pitch`      | float | 40–500 Hz   | 120     | Base pitch               |
/// | `sweep`      | float | 0–2000 Hz   | 400     | Pitch sweep start offset |
/// | `sweep_time` | float | 0.001–0.3 s | 0.03    | Pitch sweep duration     |
/// | `decay`      | float | 0.05–2.0 s  | 0.3     | Amplitude decay time     |
/// | `noise`      | float | 0.0–1.0     | 0.15    | Noise layer amount       |
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoOutput, OutputPort, TriggerInput,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_dsp::drum::{DecayEnvelope, PitchSweep};
use patches_dsp::{MonoPhaseAccumulator, xorshift64};

pub struct Tom {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    // Parameters
    pitch: f32,
    sweep_start: f32,
    sweep_time: f32,
    decay_time: f32,
    noise_amt: f32,
    // DSP state
    osc: MonoPhaseAccumulator,
    pitch_sweep: PitchSweep,
    amp_env: DecayEnvelope,
    noise_env: DecayEnvelope,
    prng_state: u64,
    // Ports
    in_trigger: TriggerInput,
    out_audio: MonoOutput,
}

impl Module for Tom {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Tom", shape.clone())
            .mono_in("trigger")
            .mono_out("out")
            .float_param("pitch", 40.0, 500.0, 120.0)
            .float_param("sweep", 0.0, 2000.0, 400.0)
            .float_param("sweep_time", 0.001, 0.3, 0.03)
            .float_param("decay", 0.05, 2.0, 0.3)
            .float_param("noise", 0.0, 1.0, 0.15)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let sr = audio_environment.sample_rate;
        let mut amp_env = DecayEnvelope::new(sr);
        amp_env.set_decay(0.3);
        let mut noise_env = DecayEnvelope::new(sr);
        noise_env.set_decay(0.01);
        let mut pitch_sweep = PitchSweep::new(sr);
        pitch_sweep.set_params(520.0, 120.0, 0.03);
        Self {
            instance_id,
            descriptor,
            sample_rate: sr,
            pitch: 120.0,
            sweep_start: 400.0,
            sweep_time: 0.03,
            decay_time: 0.3,
            noise_amt: 0.15,
            osc: MonoPhaseAccumulator::new(),
            pitch_sweep,
            amp_env,
            noise_env,
            prng_state: instance_id.as_u64() + 1,
            in_trigger: TriggerInput::default(),
            out_audio: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("pitch") {
            self.pitch = *v;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("sweep") {
            self.sweep_start = *v;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("sweep_time") {
            self.sweep_time = *v;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("decay") {
            self.decay_time = *v;
            self.amp_env.set_decay(self.decay_time);
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("noise") {
            self.noise_amt = *v;
        }
        self.pitch_sweep.set_params(self.pitch + self.sweep_start, self.pitch, self.sweep_time);
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_trigger = TriggerInput::from_ports(inputs, 0);
        self.out_audio = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let trigger_rose = self.in_trigger.tick(pool);

        if trigger_rose {
            self.osc.reset();
            self.pitch_sweep.trigger();
        }

        let freq = self.pitch_sweep.tick();
        let amp = self.amp_env.tick(trigger_rose);
        let noise_amp = self.noise_env.tick(trigger_rose);

        // Sine oscillator
        let increment = freq / self.sample_rate;
        self.osc.set_increment(increment);
        let phase = self.osc.phase;
        let sine = (phase * std::f32::consts::TAU).sin();
        self.osc.advance();

        // Noise attack texture
        let white = xorshift64(&mut self.prng_state);
        let noise = white * noise_amp * self.noise_amt;

        let output = (sine * amp) + noise;

        pool.write_mono(&self.out_audio, output);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;

    #[test]
    fn trigger_produces_output() {
        let mut h = ModuleHarness::build::<Tom>(&[]);
        h.set_mono("trigger", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        let rms = h.measure_rms(2000, "out");
        assert!(rms > 0.01, "tom should produce output, rms = {rms}");
    }

    #[test]
    fn pitch_tracking() {
        let mut h_low = ModuleHarness::build::<Tom>(&[
            ("pitch", ParameterValue::Float(60.0)),
            ("sweep", ParameterValue::Float(0.0)),
            ("noise", ParameterValue::Float(0.0)),
        ]);
        let mut h_high = ModuleHarness::build::<Tom>(&[
            ("pitch", ParameterValue::Float(300.0)),
            ("sweep", ParameterValue::Float(0.0)),
            ("noise", ParameterValue::Float(0.0)),
        ]);

        h_low.set_mono("trigger", 1.0);
        h_low.tick();
        h_low.set_mono("trigger", 0.0);
        h_high.set_mono("trigger", 1.0);
        h_high.tick();
        h_high.set_mono("trigger", 0.0);

        let low_samples = h_low.run_mono(1000, "out");
        let high_samples = h_high.run_mono(1000, "out");

        let count_crossings = |s: &[f32]| -> usize {
            s.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count()
        };

        assert!(
            count_crossings(&high_samples) > count_crossings(&low_samples),
            "higher pitch tom should have more zero crossings"
        );
    }

    #[test]
    fn output_decays() {
        let mut h = ModuleHarness::build::<Tom>(&[
            ("decay", ParameterValue::Float(0.05)),
            ("noise", ParameterValue::Float(0.0)),
        ]);
        h.set_mono("trigger", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);

        for _ in 0..22050 {
            h.tick();
        }
        let rms = h.measure_rms(100, "out");
        assert!(rms < 0.01, "tom should decay, rms = {rms}");
    }
}
