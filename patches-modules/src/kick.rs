/// 808-style kick drum synthesiser.
///
/// A sine oscillator with a fast pitch sweep from a configurable start
/// frequency down to a settable base pitch, shaped by an exponential
/// amplitude decay envelope, with optional tanh saturation for grit and
/// a transient click layer.
///
/// # Inputs
///
/// | Port      | Kind | Description                                          |
/// |-----------|------|------------------------------------------------------|
/// | `trigger` | mono | Rising edge triggers                                 |
/// | `voct`    | mono | V/oct pitch CV; overrides `sweep` start frequency if connected |
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
use crate::common::approximate::fast_exp2;
use crate::common::frequency::C0_FREQ;

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor, ModuleShape, MonoInput, MonoOutput, OutputPort, PeriodicUpdate, TriggerInput
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_dsp::drum::{DecayEnvelope, PitchSweep, saturate};
use patches_dsp::{MonoPhaseAccumulator, fast_sine};

pub struct Kick {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate_reciprocal: f32,
    // Parameters
    pitch: f32,
    sweep_start: f32,
    sweep_time: f32,
    decay_time: f32,
    drive: f32,
    click: f32,
    voct_connected: bool,
    // DSP state
    osc: MonoPhaseAccumulator,
    pitch_sweep: PitchSweep,
    amp_env: DecayEnvelope,
    click_env: DecayEnvelope,
    // Ports
    in_trigger: TriggerInput,
    voct_in : MonoInput,
    out_audio: MonoOutput,
}

impl Module for Kick {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Kick", shape.clone())
            .mono_in("trigger")
            .mono_in("voct")
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
            sample_rate_reciprocal: sr.recip(),
            pitch: 55.0,
            voct_connected: false,
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
            voct_in: MonoInput::default(),
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
        self.voct_in = MonoInput::from_ports(inputs, 1);
        let was_connected = self.voct_connected;
        self.voct_connected = self.voct_in.is_connected();
        if was_connected && !self.voct_connected {
            self.pitch_sweep.set_params(self.sweep_start, self.pitch, self.sweep_time);
        }
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
        let increment = freq * self.sample_rate_reciprocal;
        self.osc.set_increment(increment);

        // Sine oscillator
        let phase = self.osc.phase;
        let sine = fast_sine(phase);
        let two_phase = (phase + phase).fract();
        let three_phase = (two_phase + phase).fract();
        let click_signal = (fast_sine(two_phase) + fast_sine(three_phase)) * 0.5;
        self.osc.advance();

        // Mix sine body with click transient (higher harmonics)
        let signal = sine * amp + click_signal * click_amp * self.click * 0.3;

        // Apply saturation
        let output = if self.drive > 0.0 {
            saturate(signal, self.drive)
        } else {
            signal
        };

        pool.write_mono(&self.out_audio, output);
    }

    fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> {
        Some(self)
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

impl PeriodicUpdate for Kick {
    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        if !self.voct_connected {
            return;
        }
        let start_hz = C0_FREQ * fast_exp2(pool.read_mono(&self.voct_in));
        let ratio = self.pitch / self.sweep_start;
        let end_hz = start_hz * ratio;
        self.pitch_sweep.set_params(start_hz, end_hz, self.sweep_time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;

    fn make_kick() -> ModuleHarness {
        let mut h = ModuleHarness::build::<Kick>(&[
            ("pitch", ParameterValue::Float(55.0)),
            ("sweep", ParameterValue::Float(2500.0)),
            ("sweep_time", ParameterValue::Float(0.04)),
            ("decay", ParameterValue::Float(0.5)),
            ("drive", ParameterValue::Float(0.0)),
            ("click", ParameterValue::Float(0.3)),
        ]);
        h.disconnect_input("voct");
        h
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
        h.disconnect_input("voct");

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
        h_low.disconnect_input("voct");

        // High pitch kick
        let mut h_high = ModuleHarness::build::<Kick>(&[
            ("pitch", ParameterValue::Float(120.0)),
            ("sweep", ParameterValue::Float(120.0)), // No sweep
            ("sweep_time", ParameterValue::Float(0.001)),
            ("decay", ParameterValue::Float(0.5)),
            ("drive", ParameterValue::Float(0.0)),
            ("click", ParameterValue::Float(0.0)),
        ]);
        h_high.disconnect_input("voct");

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

    #[test]
    fn voct_overrides_sweep_start() {
        // voct value that maps to ~5000 Hz sweep start
        let voct_for_5000 = (5000.0f32 / 16.351_598).log2();

        // Kick with low sweep param but voct overriding to high sweep start
        let mut h_voct = ModuleHarness::build::<Kick>(&[
            ("pitch", ParameterValue::Float(55.0)),
            ("sweep", ParameterValue::Float(500.0)),
            ("sweep_time", ParameterValue::Float(0.04)),
            ("decay", ParameterValue::Float(0.5)),
            ("drive", ParameterValue::Float(0.0)),
            ("click", ParameterValue::Float(0.0)),
        ]);
        h_voct.set_mono("voct", voct_for_5000);

        // Same kick without voct — sweep starts at 500 Hz
        let mut h_no_voct = ModuleHarness::build::<Kick>(&[
            ("pitch", ParameterValue::Float(55.0)),
            ("sweep", ParameterValue::Float(500.0)),
            ("sweep_time", ParameterValue::Float(0.04)),
            ("decay", ParameterValue::Float(0.5)),
            ("drive", ParameterValue::Float(0.0)),
            ("click", ParameterValue::Float(0.0)),
        ]);
        h_no_voct.disconnect_input("voct");

        // Let periodic update run before triggering
        for _ in 0..64 { h_voct.tick(); }
        for _ in 0..64 { h_no_voct.tick(); }

        // Trigger both
        h_voct.set_mono("trigger", 1.0);
        h_voct.tick();
        h_voct.set_mono("trigger", 0.0);
        h_no_voct.set_mono("trigger", 1.0);
        h_no_voct.tick();
        h_no_voct.set_mono("trigger", 0.0);

        // Measure first 200 samples — the sweep transient
        let voct_samples = h_voct.run_mono(200, "out");
        let no_voct_samples = h_no_voct.run_mono(200, "out");

        let count_crossings = |s: &[f32]| -> usize {
            s.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count()
        };

        let voct_crossings = count_crossings(&voct_samples);
        let no_voct_crossings = count_crossings(&no_voct_samples);

        // Higher sweep start from voct should produce more crossings in the transient
        assert!(
            voct_crossings > no_voct_crossings,
            "voct sweep start at 5000 Hz should have more transient crossings than 500 Hz: voct={voct_crossings}, no_voct={no_voct_crossings}"
        );
    }
}
