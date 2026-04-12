/// Closed hi-hat synthesiser.
///
/// Metallic tone from six inharmonic square oscillators mixed with
/// highpass-filtered white noise, shaped by a short decay envelope.
///
/// # Inputs
///
/// | Port      | Kind | Description          |
/// |-----------|------|----------------------|
/// | `trigger` | mono | Rising edge triggers |
///
/// # Outputs
///
/// | Port  | Kind | Description       |
/// |-------|------|-------------------|
/// | `out` | mono | Closed hat signal |
///
/// # Parameters
///
/// | Name     | Type  | Range         | Default | Description                     |
/// |----------|-------|---------------|---------|---------------------------------|
/// | `pitch`  | float | 100–8000 Hz   | 400     | Base frequency of metallic tone |
/// | `decay`  | float | 0.005–0.2 s   | 0.04    | Amplitude decay time            |
/// | `tone`   | float | 0.0–1.0       | 0.5     | Metallic vs noise mix           |
/// | `filter` | float | 2000–16000 Hz | 8000    | Noise highpass cutoff           |
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoOutput, OutputPort, TriggerInput,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_dsp::drum::{DecayEnvelope, MetallicTone};
use patches_dsp::{SvfKernel, svf_f, q_to_damp, xorshift64};

pub struct ClosedHiHat {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    pitch: f32,
    decay_time: f32,
    tone: f32,
    filter_freq: f32,
    metallic: MetallicTone,
    amp_env: DecayEnvelope,
    hp_filter: SvfKernel,
    prng_state: u64,
    in_trigger: TriggerInput,
    out_audio: MonoOutput,
}

impl Module for ClosedHiHat {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("ClosedHiHat", shape.clone())
            .mono_in("trigger")
            .mono_out("out")
            .float_param("pitch", 100.0, 8000.0, 400.0)
            .float_param("decay", 0.005, 0.2, 0.04)
            .float_param("tone", 0.0, 1.0, 0.5)
            .float_param("filter", 2000.0, 16000.0, 8000.0)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let sr = audio_environment.sample_rate;
        let mut amp_env = DecayEnvelope::new(sr);
        amp_env.set_decay(0.04);
        let mut metallic = MetallicTone::new(sr);
        metallic.set_frequency(400.0);
        let f = svf_f(8000.0, sr);
        let d = q_to_damp(0.3);
        Self {
            instance_id,
            descriptor,
            sample_rate: sr,
            pitch: 400.0,
            decay_time: 0.04,
            tone: 0.5,
            filter_freq: 8000.0,
            metallic,
            amp_env,
            hp_filter: SvfKernel::new_static(f, d),
            prng_state: instance_id.as_u64() + 1,
            in_trigger: TriggerInput::default(),
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
            self.metallic.trigger();
        }

        let amp = self.amp_env.tick(trigger_rose);

        let metal = self.metallic.tick();
        let white = xorshift64(&mut self.prng_state);
        let (_lp, hp, _bp) = self.hp_filter.tick(white);

        let mix = metal * self.tone + hp * (1.0 - self.tone);
        let output = mix * amp;

        pool.write_mono(&self.out_audio, output);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

/// Open hi-hat synthesiser.
///
/// Same metallic tone engine as closed hi-hat but with a longer decay range.
/// Includes a `choke` input so a closed hi-hat trigger can cut it short.
///
/// # Inputs
///
/// | Port      | Kind | Description                         |
/// |-----------|------|-------------------------------------|
/// | `trigger` | mono | Rising edge triggers                |
/// | `choke`   | mono | Rising edge chokes (cuts) the sound |
///
/// # Outputs
///
/// | Port  | Kind | Description     |
/// |-------|------|-----------------|
/// | `out` | mono | Open hat signal |
///
/// # Parameters
///
/// | Name     | Type  | Range         | Default | Description                     |
/// |----------|-------|---------------|---------|---------------------------------|
/// | `pitch`  | float | 100–8000 Hz   | 400     | Base frequency of metallic tone |
/// | `decay`  | float | 0.05–4.0 s    | 0.5     | Amplitude decay time            |
/// | `tone`   | float | 0.0–1.0       | 0.5     | Metallic vs noise mix           |
/// | `filter` | float | 2000–16000 Hz | 8000    | Noise highpass cutoff           |
pub struct OpenHiHat {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    pitch: f32,
    decay_time: f32,
    tone: f32,
    filter_freq: f32,
    metallic: MetallicTone,
    amp_env: DecayEnvelope,
    hp_filter: SvfKernel,
    prng_state: u64,
    in_trigger: TriggerInput,
    in_choke: TriggerInput,
    out_audio: MonoOutput,
}

impl Module for OpenHiHat {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("OpenHiHat", shape.clone())
            .mono_in("trigger")
            .mono_in("choke")
            .mono_out("out")
            .float_param("pitch", 100.0, 8000.0, 400.0)
            .float_param("decay", 0.05, 4.0, 0.5)
            .float_param("tone", 0.0, 1.0, 0.5)
            .float_param("filter", 2000.0, 16000.0, 8000.0)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let sr = audio_environment.sample_rate;
        let mut amp_env = DecayEnvelope::new(sr);
        amp_env.set_decay(0.5);
        let mut metallic = MetallicTone::new(sr);
        metallic.set_frequency(400.0);
        let f = svf_f(8000.0, sr);
        let d = q_to_damp(0.3);
        Self {
            instance_id,
            descriptor,
            sample_rate: sr,
            pitch: 400.0,
            decay_time: 0.5,
            tone: 0.5,
            filter_freq: 8000.0,
            metallic,
            amp_env,
            hp_filter: SvfKernel::new_static(f, d),
            prng_state: instance_id.as_u64() + 1,
            in_trigger: TriggerInput::default(),
            in_choke: TriggerInput::default(),
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
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_trigger = TriggerInput::from_ports(inputs, 0);
        self.in_choke = TriggerInput::from_ports(inputs, 1);
        self.out_audio = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let trigger_rose = self.in_trigger.tick(pool);
        let choke_rose = self.in_choke.tick(pool);

        if trigger_rose {
            self.metallic.trigger();
        }

        if choke_rose {
            self.amp_env.choke();
        }

        let amp = self.amp_env.tick(trigger_rose);

        let metal = self.metallic.tick();
        let white = xorshift64(&mut self.prng_state);
        let (_lp, hp, _bp) = self.hp_filter.tick(white);

        let mix = metal * self.tone + hp * (1.0 - self.tone);
        let output = mix * amp;

        pool.write_mono(&self.out_audio, output);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;

    #[test]
    fn closed_hihat_trigger_produces_output() {
        let mut h = ModuleHarness::build::<ClosedHiHat>(&[]);
        h.set_mono("trigger", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        let rms = h.measure_rms(500, "out");
        assert!(rms > 0.001, "closed hihat should produce output, rms = {rms}");
    }

    #[test]
    fn closed_hihat_short_decay() {
        let mut h = ModuleHarness::build::<ClosedHiHat>(&[
            ("decay", ParameterValue::Float(0.01)),
        ]);
        h.set_mono("trigger", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        // After 0.1s (well past 0.01s decay)
        for _ in 0..4410 {
            h.tick();
        }
        let rms = h.measure_rms(100, "out");
        assert!(rms < 0.01, "closed hihat should decay quickly, rms = {rms}");
    }

    #[test]
    fn open_hihat_trigger_produces_output() {
        let mut h = ModuleHarness::build::<OpenHiHat>(&[]);
        h.set_mono("trigger", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        let rms = h.measure_rms(2000, "out");
        assert!(rms > 0.001, "open hihat should produce output, rms = {rms}");
    }

    #[test]
    fn open_hihat_choke_silences() {
        let mut h = ModuleHarness::build::<OpenHiHat>(&[
            ("decay", ParameterValue::Float(2.0)),
        ]);
        // Trigger
        h.set_mono("trigger", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);

        // Let it ring for a bit
        for _ in 0..500 {
            h.tick();
        }

        // Verify it's still producing output
        let rms_before = h.measure_rms(100, "out");
        assert!(rms_before > 0.001, "should still be ringing before choke");

        // Choke
        h.set_mono("choke", 1.0);
        h.tick();
        h.set_mono("choke", 0.0);

        // Should be silent
        let rms_after = h.measure_rms(100, "out");
        assert!(rms_after < 0.001, "should be silent after choke, rms = {rms_after}");
    }

    #[test]
    fn open_hihat_longer_than_closed() {
        let mut h_closed = ModuleHarness::build::<ClosedHiHat>(&[
            ("decay", ParameterValue::Float(0.04)),
        ]);
        let mut h_open = ModuleHarness::build::<OpenHiHat>(&[
            ("decay", ParameterValue::Float(0.5)),
        ]);

        // Trigger both
        h_closed.set_mono("trigger", 1.0);
        h_closed.tick();
        h_closed.set_mono("trigger", 0.0);
        h_open.set_mono("trigger", 1.0);
        h_open.tick();
        h_open.set_mono("trigger", 0.0);

        // Measure RMS at 0.1s
        for _ in 0..4410 {
            h_closed.tick();
            h_open.tick();
        }

        let rms_closed = h_closed.measure_rms(200, "out");
        let rms_open = h_open.measure_rms(200, "out");
        assert!(
            rms_open > rms_closed,
            "open hat should ring longer: open={rms_open}, closed={rms_closed}"
        );
    }
}
