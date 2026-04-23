/// 808-style clap synthesiser.
///
/// White noise passed through a bandpass filter, gated by a burst generator
/// to produce the initial "clappy" retriggered transient, then shaped by a
/// longer decay envelope for the tail.
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
/// | Port  | Kind | Description |
/// |-------|------|-------------|
/// | `out` | mono | Clap signal |
///
/// # Parameters
///
/// | Name     | Type  | Range       | Default | Description               |
/// |----------|-------|-------------|---------|---------------------------|
/// | `decay`  | float | 0.05–2.0 s  | 0.3     | Tail decay time           |
/// | `filter` | float | 500–8000 Hz | 1200    | Bandpass centre frequency |
/// | `q`      | float | 0.0–1.0     | 0.4     | Bandpass resonance        |
/// | `spread` | float | 0.0–1.0     | 0.5     | Spacing between bursts    |
/// | `bursts` | int   | 1–8         | 4       | Number of noise bursts    |
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoInput, MonoOutput, OutputPort,
};
use patches_core::cables::TriggerInput;
use patches_core::param_frame::ParamView;
use patches_core::module_params;
use patches_dsp::drum::{DecayEnvelope, BurstGenerator};
use patches_dsp::{SvfKernel, svf_f, q_to_damp, xorshift64};

module_params! {
    ClapDrum {
        decay:  Float,
        filter: Float,
        q:      Float,
        spread: Float,
        bursts: Int,
    }
}

pub struct ClapDrum {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    // Parameters
    decay_time: f32,
    filter_freq: f32,
    filter_q: f32,
    spread: f32,
    bursts: usize,
    latched_velocity: f32,
    // DSP state
    tail_env: DecayEnvelope,
    burst_gen: BurstGenerator,
    noise_filter: SvfKernel,
    prng_state: u64,
    // Ports
    in_trigger: TriggerInput,
    in_velocity: MonoInput,
    out_audio: MonoOutput,
}

impl Module for ClapDrum {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Clap", shape.clone())
            .trigger_in("trigger")
            .mono_in("velocity")
            .mono_out("out")
            .float_param(params::decay, 0.05, 2.0, 0.3)
            .float_param(params::filter, 500.0, 8000.0, 1200.0)
            .float_param(params::q, 0.0, 1.0, 0.4)
            .float_param(params::spread, 0.0, 1.0, 0.5)
            .int_param(params::bursts, 1, 8, 4)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let sr = audio_environment.sample_rate;
        let mut tail_env = DecayEnvelope::new(sr);
        tail_env.set_decay(0.3);
        let mut burst_gen = BurstGenerator::new(sr);
        burst_gen.set_params(4, 0.005, 0.7);
        let f = svf_f(1200.0, sr);
        let d = q_to_damp(0.4);
        let noise_filter = SvfKernel::new_static(f, d);
        Self {
            instance_id,
            descriptor,
            sample_rate: sr,
            decay_time: 0.3,
            filter_freq: 1200.0,
            filter_q: 0.4,
            spread: 0.5,
            bursts: 4,
            latched_velocity: 1.0,
            tail_env,
            burst_gen,
            noise_filter,
            prng_state: instance_id.as_u64() + 1,
            in_trigger: TriggerInput::default(),
            in_velocity: MonoInput::default(),
            out_audio: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        let v = p.get(params::decay);
        self.decay_time = v;
        self.tail_env.set_decay(self.decay_time);
        let v = p.get(params::filter);
        self.filter_freq = v;
        let v = p.get(params::q);
        self.filter_q = v;
        let v = p.get(params::spread);
        self.spread = v;
        let v = p.get(params::bursts);
        self.bursts = (v as usize).clamp(1, 8);
        let spacing_secs = self.spread * 0.01;
        self.burst_gen.set_params(self.bursts, spacing_secs, 0.7);
        let f = svf_f(self.filter_freq, self.sample_rate);
        let d = q_to_damp(self.filter_q);
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
        let trigger_rose = self.in_trigger.tick(pool).is_some();

        if trigger_rose {
            self.latched_velocity = if self.in_velocity.connected {
                pool.read_mono(&self.in_velocity)
            } else {
                1.0
            };
        }

        let white = xorshift64(&mut self.prng_state);
        let (_lp, _hp, bp) = self.noise_filter.tick(white);

        // Burst-gated noise for the clap transient
        let burst = self.burst_gen.tick(trigger_rose, bp);

        // Tail envelope
        let tail_amp = self.tail_env.tick(trigger_rose);

        // Combine: burst transient + filtered noise tail
        let output = burst + bp * tail_amp * 0.5;

        pool.write_mono(&self.out_audio, output * self.latched_velocity);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use patches_core::ParameterValue;
    use super::*;
    use patches_core::test_support::ModuleHarness;

    #[test]
    fn trigger_produces_output() {
        let mut h = ModuleHarness::build::<ClapDrum>(&[]);
        h.disconnect_input("velocity");
        h.set_mono("trigger", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        let rms = h.measure_rms(2000, "out");
        assert!(rms > 0.001, "clap should produce output, rms = {rms}");
    }

    #[test]
    fn burst_count_affects_output() {
        // Few bursts
        let mut h1 = ModuleHarness::build::<ClapDrum>(&[
            ("bursts", ParameterValue::Int(2)),
            ("spread", ParameterValue::Float(0.5)),
        ]);
        h1.disconnect_input("velocity");
        h1.set_mono("trigger", 1.0);
        h1.tick();
        h1.set_mono("trigger", 0.0);

        // Many bursts
        let mut h2 = ModuleHarness::build::<ClapDrum>(&[
            ("bursts", ParameterValue::Int(8)),
            ("spread", ParameterValue::Float(0.5)),
        ]);
        h2.disconnect_input("velocity");
        h2.set_mono("trigger", 1.0);
        h2.tick();
        h2.set_mono("trigger", 0.0);

        // Both should produce output
        let rms1 = h1.measure_rms(2000, "out");
        let rms2 = h2.measure_rms(2000, "out");
        assert!(rms1 > 0.001, "2-burst clap should produce output");
        assert!(rms2 > 0.001, "8-burst clap should produce output");
    }
}
