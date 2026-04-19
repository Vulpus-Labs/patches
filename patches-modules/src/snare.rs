/// 808-style snare drum synthesiser.
///
/// Combines a tuned body oscillator (sine with short pitch sweep) with a
/// bandpass-filtered noise burst. Each path has its own decay envelope;
/// the `tone` parameter crossfades between them.
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
/// | Port  | Kind | Description  |
/// |-------|------|--------------|
/// | `out` | mono | Snare signal |
///
/// # Parameters
///
/// | Name         | Type  | Range        | Default | Description                      |
/// |--------------|-------|--------------|---------|----------------------------------|
/// | `pitch`      | float | 80–400 Hz    | 180     | Body oscillator base pitch       |
/// | `tone`       | float | 0.0–1.0      | 0.5     | Body vs noise mix (0 = all body) |
/// | `body_decay` | float | 0.01–1.0 s   | 0.15    | Body amplitude decay time        |
/// | `noise_decay`| float | 0.01–1.0 s   | 0.2     | Noise amplitude decay time       |
/// | `noise_freq` | float | 500–10000 Hz | 3000    | Noise bandpass centre frequency  |
/// | `noise_q`    | float | 0.0–1.0      | 0.3     | Noise bandpass resonance         |
/// | `snap`       | float | 0.0–1.0      | 0.5     | Transient snap intensity         |
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoInput, MonoOutput, OutputPort, TriggerInput,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_dsp::drum::{DecayEnvelope, PitchSweep};
use patches_dsp::{MonoPhaseAccumulator, SvfKernel, svf_f, q_to_damp, xorshift64};

pub struct Snare {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    // Parameters
    pitch: f32,
    tone: f32,
    body_decay_time: f32,
    noise_decay_time: f32,
    noise_freq: f32,
    noise_q: f32,
    snap: f32,
    latched_velocity: f32,
    // DSP state
    osc: MonoPhaseAccumulator,
    pitch_sweep: PitchSweep,
    body_env: DecayEnvelope,
    noise_env: DecayEnvelope,
    snap_env: DecayEnvelope,
    noise_filter: SvfKernel,
    prng_state: u64,
    // Ports
    in_trigger: TriggerInput,
    in_velocity: MonoInput,
    out_audio: MonoOutput,
}

impl Module for Snare {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Snare", shape.clone())
            .mono_in("trigger")
            .mono_in("velocity")
            .mono_out("out")
            .float_param("pitch", 80.0, 400.0, 180.0)
            .float_param("tone", 0.0, 1.0, 0.5)
            .float_param("body_decay", 0.01, 1.0, 0.15)
            .float_param("noise_decay", 0.01, 1.0, 0.2)
            .float_param("noise_freq", 500.0, 10000.0, 3000.0)
            .float_param("noise_q", 0.0, 1.0, 0.3)
            .float_param("snap", 0.0, 1.0, 0.5)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let sr = audio_environment.sample_rate;
        let mut body_env = DecayEnvelope::new(sr);
        body_env.set_decay(0.15);
        let mut noise_env = DecayEnvelope::new(sr);
        noise_env.set_decay(0.2);
        let mut snap_env = DecayEnvelope::new(sr);
        snap_env.set_decay(0.005);
        let mut pitch_sweep = PitchSweep::new(sr);
        pitch_sweep.set_params(360.0, 180.0, 0.02);
        let f = svf_f(3000.0, sr);
        let d = q_to_damp(0.3);
        let noise_filter = SvfKernel::new_static(f, d);
        Self {
            instance_id,
            descriptor,
            sample_rate: sr,
            pitch: 180.0,
            tone: 0.5,
            body_decay_time: 0.15,
            noise_decay_time: 0.2,
            noise_freq: 3000.0,
            noise_q: 0.3,
            snap: 0.5,
            latched_velocity: 1.0,
            osc: MonoPhaseAccumulator::new(),
            pitch_sweep,
            body_env,
            noise_env,
            snap_env,
            noise_filter,
            prng_state: instance_id.as_u64() + 1,
            in_trigger: TriggerInput::default(),
            in_velocity: MonoInput::default(),
            out_audio: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("pitch") {
            self.pitch = *v;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("tone") {
            self.tone = *v;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("body_decay") {
            self.body_decay_time = *v;
            self.body_env.set_decay(self.body_decay_time);
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("noise_decay") {
            self.noise_decay_time = *v;
            self.noise_env.set_decay(self.noise_decay_time);
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("noise_freq") {
            self.noise_freq = *v;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("noise_q") {
            self.noise_q = *v;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("snap") {
            self.snap = *v;
        }
        self.pitch_sweep.set_params(self.pitch * 2.0, self.pitch, 0.02);
        let f = svf_f(self.noise_freq, self.sample_rate);
        let d = q_to_damp(self.noise_q);
        self.noise_filter = SvfKernel::new_static(f, d);
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
            self.osc.reset();
            self.pitch_sweep.trigger();
        }

        let freq = self.pitch_sweep.tick();
        let body_amp = self.body_env.tick(trigger_rose);
        let noise_amp = self.noise_env.tick(trigger_rose);
        let snap_amp = self.snap_env.tick(trigger_rose);

        // Body: sine oscillator
        let increment = freq / self.sample_rate;
        self.osc.set_increment(increment);
        let phase = self.osc.phase;
        let sine = (phase * std::f32::consts::TAU).sin();
        self.osc.advance();

        let body = sine * body_amp;

        // Noise: white noise through bandpass SVF
        let white = xorshift64(&mut self.prng_state);
        let (_lp, _hp, bp) = self.noise_filter.tick(white);
        let noise = bp * noise_amp;

        // Mix with tone crossfade, add snap transient
        let mix = body * (1.0 - self.tone) + noise * self.tone;
        let output = mix + white * snap_amp * self.snap * 0.5;

        pool.write_mono(&self.out_audio, output * self.latched_velocity);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;

    fn make_snare() -> ModuleHarness {
        let mut h = ModuleHarness::build::<Snare>(&[]);
        h.disconnect_input("velocity");
        h
    }

    #[test]
    fn trigger_produces_non_silent_output() {
        let mut h = make_snare();
        h.set_mono("trigger", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        let rms = h.measure_rms(2000, "out");
        assert!(rms > 0.01, "snare should produce output, rms = {rms}");
    }

    #[test]
    fn output_decays() {
        let mut h = ModuleHarness::build::<Snare>(&[
            ("body_decay", ParameterValue::Float(0.05)),
            ("noise_decay", ParameterValue::Float(0.05)),
        ]);
        h.disconnect_input("velocity");
        h.set_mono("trigger", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);

        // Let it decay
        for _ in 0..22050 {
            h.tick();
        }
        let rms = h.measure_rms(100, "out");
        assert!(rms < 0.02, "snare should decay, rms = {rms}");
    }

    #[test]
    fn velocity_scales_output() {
        let mut h_full = make_snare();
        h_full.set_mono("trigger", 1.0);
        h_full.tick();
        h_full.set_mono("trigger", 0.0);
        let rms_full = h_full.measure_rms(2000, "out");

        let mut h_half = ModuleHarness::build::<Snare>(&[]);
        h_half.set_mono("velocity", 0.5);
        h_half.set_mono("trigger", 1.0);
        h_half.tick();
        h_half.set_mono("trigger", 0.0);
        let rms_half = h_half.measure_rms(2000, "out");

        let ratio = rms_half / rms_full;
        assert!(
            (ratio - 0.5).abs() < 0.1,
            "half velocity should roughly halve output: ratio = {ratio}"
        );
    }

    #[test]
    fn tone_extremes() {
        // All body (tone=0)
        let mut h_body = ModuleHarness::build::<Snare>(&[
            ("tone", ParameterValue::Float(0.0)),
            ("snap", ParameterValue::Float(0.0)),
        ]);
        h_body.disconnect_input("velocity");
        h_body.set_mono("trigger", 1.0);
        h_body.tick();
        h_body.set_mono("trigger", 0.0);
        let body_samples = h_body.run_mono(2000, "out");

        // All noise (tone=1)
        let mut h_noise = ModuleHarness::build::<Snare>(&[
            ("tone", ParameterValue::Float(1.0)),
            ("snap", ParameterValue::Float(0.0)),
        ]);
        h_noise.disconnect_input("velocity");
        h_noise.set_mono("trigger", 1.0);
        h_noise.tick();
        h_noise.set_mono("trigger", 0.0);
        let noise_samples = h_noise.run_mono(2000, "out");

        // Both should produce output
        let body_rms: f32 = (body_samples.iter().map(|x| x * x).sum::<f32>() / 2000.0).sqrt();
        let noise_rms: f32 = (noise_samples.iter().map(|x| x * x).sum::<f32>() / 2000.0).sqrt();
        assert!(body_rms > 0.01, "body path should produce output");
        assert!(noise_rms > 0.01, "noise path should produce output");
    }
}
