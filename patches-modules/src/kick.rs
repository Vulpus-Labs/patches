/// 808-style kick drum synthesiser.
///
/// A sine oscillator with a fast pitch sweep from a configurable start
/// frequency down to a settable base pitch, shaped by an exponential
/// amplitude decay envelope, with optional tanh saturation for grit and
/// a transient click layer.
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
/// | `out` | mono | Kick signal |
///
/// # Parameters
///
/// | Name         | Type  | Range       | Default | Description                       |
/// |--------------|-------|-------------|---------|-----------------------------------|
/// | `pitch`      | float | 20–200 Hz   | 55      | Base pitch of the kick            |
/// | `sweep`      | float | 0–5000 Hz   | 2500    | Starting frequency of pitch sweep |
/// | `sweep_time` | float | 0.001–0.5 s | 0.04    | Duration of pitch sweep           |
/// | `decay`      | float | 0.01–2.0 s  | 0.5     | Amplitude decay time              |
/// | `drive`      | float | 0.0–1.0     | 0.0     | Saturation amount                 |
/// | `click`      | float | 0.0–1.0     | 0.3     | Transient click intensity         |
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoOutput, OutputPort, TriggerInput,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_dsp::drum::{DecayEnvelope, PitchSweep, saturate};
use patches_dsp::MonoPhaseAccumulator;

pub struct Kick {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    // Parameters
    pitch: f32,
    sweep_start: f32,
    sweep_time: f32,
    decay_time: f32,
    drive: f32,
    click: f32,
    // DSP state
    osc: MonoPhaseAccumulator,
    pitch_sweep: PitchSweep,
    amp_env: DecayEnvelope,
    click_env: DecayEnvelope,
    // Ports
    in_trigger: TriggerInput,
    out_audio: MonoOutput,
}

impl Module for Kick {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Kick", shape.clone())
            .mono_in("trigger")
            .mono_out("out")
            .float_param("pitch", 20.0, 200.0, 55.0)
            .float_param("sweep", 0.0, 5000.0, 2500.0)
            .float_param("sweep_time", 0.001, 0.5, 0.04)
            .float_param("decay", 0.01, 2.0, 0.5)
            .float_param("drive", 0.0, 1.0, 0.0)
            .float_param("click", 0.0, 1.0, 0.3)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let sr = audio_environment.sample_rate;
        let mut amp_env = DecayEnvelope::new(sr);
        amp_env.set_decay(0.5);
        let mut click_env = DecayEnvelope::new(sr);
        click_env.set_decay(0.003);
        let mut pitch_sweep = PitchSweep::new(sr);
        pitch_sweep.set_params(2500.0, 55.0, 0.04);
        Self {
            instance_id,
            descriptor,
            sample_rate: sr,
            pitch: 55.0,
            sweep_start: 2500.0,
            sweep_time: 0.04,
            decay_time: 0.5,
            drive: 0.0,
            click: 0.3,
            osc: MonoPhaseAccumulator::new(),
            pitch_sweep,
            amp_env,
            click_env,
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
        if let Some(ParameterValue::Float(v)) = params.get_scalar("drive") {
            self.drive = *v;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("click") {
            self.click = *v;
        }
        self.pitch_sweep.set_params(self.sweep_start, self.pitch, self.sweep_time);
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
        let click_amp = self.click_env.tick(trigger_rose);

        // Set oscillator frequency
        let increment = freq / self.sample_rate;
        self.osc.set_increment(increment);

        // Sine oscillator
        let phase = self.osc.phase;
        let sine = (phase * std::f32::consts::TAU).sin();
        self.osc.advance();

        // Mix sine body with click transient (higher harmonics)
        let click_signal = (phase * std::f32::consts::TAU * 2.0).sin()
            + (phase * std::f32::consts::TAU * 3.0).sin() * 0.5;
        let signal = sine * amp + click_signal * click_amp * self.click * 0.3;

        // Apply saturation
        let output = if self.drive > 0.0 {
            saturate(signal, self.drive)
        } else {
            signal
        };

        pool.write_mono(&self.out_audio, output);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;

    fn make_kick() -> ModuleHarness {
        ModuleHarness::build::<Kick>(&[
            ("pitch", ParameterValue::Float(55.0)),
            ("sweep", ParameterValue::Float(2500.0)),
            ("sweep_time", ParameterValue::Float(0.04)),
            ("decay", ParameterValue::Float(0.5)),
            ("drive", ParameterValue::Float(0.0)),
            ("click", ParameterValue::Float(0.3)),
        ])
    }

    #[test]
    fn trigger_produces_non_silent_output() {
        let mut h = make_kick();
        h.set_mono("trigger", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        let rms = h.measure_rms(2000, "out");
        assert!(rms > 0.01, "kick should produce audible output, rms = {rms}");
    }

    #[test]
    fn output_decays_to_near_zero() {
        let mut h = ModuleHarness::build::<Kick>(&[
            ("pitch", ParameterValue::Float(55.0)),
            ("decay", ParameterValue::Float(0.1)),
            ("sweep", ParameterValue::Float(2500.0)),
            ("sweep_time", ParameterValue::Float(0.04)),
            ("drive", ParameterValue::Float(0.0)),
            ("click", ParameterValue::Float(0.3)),
        ]);

        // Trigger
        h.set_mono("trigger", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);

        // Let it decay for 0.5s (well past 0.1s decay)
        for _ in 0..22050 {
            h.tick();
        }

        // Last 100 samples should be near zero
        let rms = h.measure_rms(100, "out");
        assert!(rms < 0.01, "kick should decay to near zero, rms = {rms}");
    }

    #[test]
    fn pitch_parameter_affects_output() {
        // Low pitch kick
        let mut h_low = ModuleHarness::build::<Kick>(&[
            ("pitch", ParameterValue::Float(40.0)),
            ("sweep", ParameterValue::Float(40.0)), // No sweep
            ("sweep_time", ParameterValue::Float(0.001)),
            ("decay", ParameterValue::Float(0.5)),
            ("drive", ParameterValue::Float(0.0)),
            ("click", ParameterValue::Float(0.0)),
        ]);

        // High pitch kick
        let mut h_high = ModuleHarness::build::<Kick>(&[
            ("pitch", ParameterValue::Float(120.0)),
            ("sweep", ParameterValue::Float(120.0)), // No sweep
            ("sweep_time", ParameterValue::Float(0.001)),
            ("decay", ParameterValue::Float(0.5)),
            ("drive", ParameterValue::Float(0.0)),
            ("click", ParameterValue::Float(0.0)),
        ]);

        // Trigger both
        h_low.set_mono("trigger", 1.0);
        h_low.tick();
        h_low.set_mono("trigger", 0.0);
        h_high.set_mono("trigger", 1.0);
        h_high.tick();
        h_high.set_mono("trigger", 0.0);

        // Count zero-crossings over 1000 samples as a proxy for frequency
        let low_samples = h_low.run_mono(1000, "out");
        let high_samples = h_high.run_mono(1000, "out");

        let count_crossings = |s: &[f32]| -> usize {
            s.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count()
        };

        let low_crossings = count_crossings(&low_samples);
        let high_crossings = count_crossings(&high_samples);
        assert!(
            high_crossings > low_crossings,
            "higher pitch should have more zero crossings: low={low_crossings}, high={high_crossings}"
        );
    }
}
