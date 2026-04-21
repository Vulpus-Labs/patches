use patches_core::{
    params_enum,
    AudioEnvironment, BoundedRandomWalk, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort, GLOBAL_DRIFT, HALF_SEMITONE_VOCT,
    OSCILLATOR_DRIFT_STEP,
};
use patches_core::module_params;
use patches_core::param_frame::ParamView;

params_enum! {
    pub enum OscFmType {
        Linear => "linear",
        Logarithmic => "logarithmic",
    }
}

module_params! {
    Oscillator {
        frequency: Float,
        fm_type:   Enum<OscFmType>,
        drift:     Float,
    }
}

use patches_dsp::polyblep;
use crate::common::approximate::lookup_sine;
use crate::common::frequency::{C0_FREQ, FMMode, MonoFrequencyConverter, MonoFrequencyChangeTracker};
use crate::common::phase_accumulator::MonoPhaseAccumulator;

/// Number of samples between drift state updates for the per-instance drift random walk.
const DRIFT_PERIOD: u8 = 64;

/// A multi-waveform oscillator driven by a single phase accumulator.
///
/// Outputs sine, triangle, sawtooth, and square waveforms simultaneously.
/// All share the same phase; only connected outputs are computed each sample.
/// The `frequency` parameter is a V/OCT offset from C0 (≈ 16.35 Hz):
/// `0.0` → C0, `1.0` → C1, `4.0` → C4 (middle C). Applied before any `voct` CV.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `voct` | mono | V/oct pitch CV added to base frequency |
/// | `fm` | mono | Frequency modulation input |
/// | `pulse_width_cv` | mono | Pulse width modulation for the square output |
/// | `phase_mod` | mono | Phase modulation offset applied to all waveforms |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `sine` | mono | Sine waveform |
/// | `triangle` | mono | Triangle waveform |
/// | `sawtooth` | mono | Sawtooth waveform (PolyBLEP anti-aliased) |
/// | `square` | mono | Square waveform (PolyBLEP anti-aliased, PWM via `pulse_width_cv`) |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `frequency` | float | -4.0 -- 12.0 | `0.0` | Base pitch as V/oct offset from C0 |
/// | `fm_type` | enum | linear, logarithmic | `linear` | FM modulation mode |
/// | `drift` | float | 0.0 -- 1.0 | `0.0` | Pitch drift amount (per-instance random walk + global drift) |
pub struct Oscillator {
    instance_id: InstanceId,
    phase_acc: MonoPhaseAccumulator,
    freq_converter: MonoFrequencyConverter,
    freq_tracker: MonoFrequencyChangeTracker,
    descriptor: ModuleDescriptor,
    // Input port fields
    in_voct: MonoInput,
    in_fm: MonoInput,
    in_pulse_width: MonoInput,
    in_phase_mod: MonoInput,
    /// Fixed input pointing at the engine-level global drift backplane slot.
    in_global_drift: MonoInput,
    // Output port fields
    out_sine: MonoOutput,
    out_triangle: MonoOutput,
    out_sawtooth: MonoOutput,
    out_square: MonoOutput,
    // Drift state
    /// `drift` parameter value in [0.0, 1.0]. Zero disables drift entirely.
    drift: f32,
    /// Per-instance random walk for local pitch drift.
    drift_walk: BoundedRandomWalk,
    /// Counts samples since last drift update; resets to 0 every `DRIFT_PERIOD`.
    drift_counter: u8,
    /// Current V/OCT offset added to the voct input each frequency calculation.
    drift_voct_offset: f32,
}

impl Module for Oscillator {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Osc", shape.clone())
            .mono_in("voct")
            .mono_in("fm")
            .mono_in("pulse_width_cv")
            .mono_in("phase_mod")
            .mono_out("sine")
            .mono_out("triangle")
            .mono_out("sawtooth")
            .mono_out("square")
            .float_param(params::frequency, -4.0, 12.0, 0.0)
            .enum_param(params::fm_type, OscFmType::Linear)
            .float_param(params::drift, 0.0, 1.0, 0.0)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        // Derive a non-zero seed from instance_id so each oscillator drifts independently.
        let seed = (instance_id.as_u64().wrapping_add(1)) as u32;
        Self {
            instance_id,
            phase_acc: MonoPhaseAccumulator::new(),
            freq_converter: MonoFrequencyConverter::new(audio_environment.sample_rate),
            freq_tracker: MonoFrequencyChangeTracker::new(C0_FREQ),
            descriptor,
            in_voct: MonoInput::default(),
            in_fm: MonoInput::default(),
            in_pulse_width: MonoInput::default(),
            in_phase_mod: MonoInput::default(),
            in_global_drift: MonoInput { cable_idx: GLOBAL_DRIFT, scale: 1.0, connected: true },
            out_sine: MonoOutput::default(),
            out_triangle: MonoOutput::default(),
            out_sawtooth: MonoOutput::default(),
            out_square: MonoOutput::default(),
            drift: 0.0,
            drift_walk: BoundedRandomWalk::new(seed, OSCILLATOR_DRIFT_STEP),
            drift_counter: 0,
            drift_voct_offset: 0.0,
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        let v = p.get(params::frequency);
        self.freq_tracker.set_voct_offset(v);
        let inc = self.freq_converter.to_increment(self.freq_tracker.base_frequency());
        self.phase_acc.set_increment(inc);
        let t: OscFmType = p.get(params::fm_type);
        let fm_mode = match t {
            OscFmType::Linear => FMMode::Linear,
            OscFmType::Logarithmic => FMMode::Exponential,
        };
        self.freq_tracker.set_fm_mode(fm_mode);
        let v = p.get(params::drift);
        self.drift = v;
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_voct = inputs[0].expect_mono();
        self.in_fm = inputs[1].expect_mono();
        self.in_pulse_width = inputs[2].expect_mono();
        self.in_phase_mod = inputs[3].expect_mono();
        self.out_sine = outputs[0].expect_mono();
        self.out_triangle = outputs[1].expect_mono();
        self.out_sawtooth = outputs[2].expect_mono();
        self.out_square = outputs[3].expect_mono();
        
        self.freq_tracker.voct_modulating = self.in_voct.is_connected();
        self.freq_tracker.fm_modulating = self.in_fm.is_connected();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let phase = self.phase_acc.phase;
        let read_phase = if self.in_phase_mod.is_connected() {
            (phase + pool.read_mono(&self.in_phase_mod)).rem_euclid(1.0)
        } else {
            phase
        };

        if self.out_sine.is_connected() {
            pool.write_mono(&self.out_sine, lookup_sine(read_phase));
        }
        if self.out_triangle.is_connected() {
            pool.write_mono(&self.out_triangle, 1.0 - 4.0 * (read_phase - 0.5).abs());
        }
        if self.out_sawtooth.is_connected() {
            let dt = self.phase_acc.phase_increment;
            pool.write_mono(&self.out_sawtooth, (2.0 * read_phase - 1.0) - polyblep(read_phase, dt));
        }
        if self.out_square.is_connected() {
            let dt = self.phase_acc.phase_increment;
            let duty = if self.in_pulse_width.is_connected() {
                (0.5 + 0.5 * pool.read_mono(&self.in_pulse_width)).clamp(0.01, 0.99)
            } else {
                0.5
            };
            let raw = if read_phase < duty { 1.0 } else { -1.0 };
            let blep = polyblep(read_phase, dt)
                - polyblep((read_phase - duty).rem_euclid(1.0), dt);
            pool.write_mono(&self.out_square, raw + blep);
        }

        // Drift: every DRIFT_PERIOD samples, advance the local walk and sample
        // the engine-level global drift, then recompute frequency if needed.
        let force_recalc = if self.drift > 0.0 {
            self.drift_counter = self.drift_counter.wrapping_add(1);
            if self.drift_counter >= DRIFT_PERIOD {
                self.drift_counter = 0;
                let global_val = pool.read_mono(&self.in_global_drift);
                let local_val = self.drift_walk.advance();
                // Each component is in [-1, 1]; scale sum so combined max = ±HALF_SEMITONE_VOCT.
                self.drift_voct_offset = (global_val + local_val) * (HALF_SEMITONE_VOCT * 0.5) * self.drift;
                true
            } else {
                false
            }
        } else {
            false
        };

        if self.freq_tracker.is_modulating() {
            let voct = pool.read_mono(&self.in_voct) + self.drift_voct_offset;
            let fm = pool.read_mono(&self.in_fm);
            let freq = self.freq_tracker.compute_modulated(voct, fm);
            self.phase_acc.set_increment(self.freq_converter.to_increment(freq));
        } else if force_recalc {
            // No voct/fm modulation but drift changed: recompute from base frequency.
            let freq = self.freq_tracker.base_frequency() * self.drift_voct_offset.exp2();
            self.phase_acc.set_increment(self.freq_converter.to_increment(freq));
        }
        self.phase_acc.advance();
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::approximate::lookup_sine;
    use crate::common::frequency::C0_FREQ;
    use patches_core::{AudioEnvironment, CableValue};
    use patches_core::test_support::{assert_within, ModuleHarness, params};

    fn env(sample_rate: f32) -> AudioEnvironment {
        AudioEnvironment { sample_rate, poly_voices: 16, periodic_update_interval: 32, hosted: false }
    }

    fn make_osc(frequency: f32, sample_rate: f32) -> ModuleHarness {
        let mut h = ModuleHarness::build_with_env::<Oscillator>(
            params!["frequency" => frequency],
            env(sample_rate),
        );
        // Most tests don't use CV inputs; disconnect all inputs by default.
        h.disconnect_all_inputs();
        h
    }

    #[test]
    fn sine_output_peak_at_quarter_cycle() {
        // At sample_rate = C0*100, each tick advances phase by 1/100.
        // The 26th tick processes phase 0.25 (quarter-period), where sine peaks.
        let period = 100_usize;
        let mut h = make_osc(0.0, C0_FREQ * period as f32);
        let samples = h.run_mono(26, "sine");
        // lookup table max error ~1e-4; 1e-3 gives headroom for f32 phase accumulation
        assert_within!(lookup_sine(0.25), *samples.last().unwrap(), 1e-3_f32);
    }

    #[test]
    fn triangle_output_peak_and_trough_correct() {
        // triangle = 1.0 - 4.0 * (phase - 0.5).abs()
        // phase 0.0 → trough = -1.0; phase 0.5 → peak = +1.0.
        let period = 100_usize;
        let mut h = make_osc(0.0, C0_FREQ * period as f32);
        let samples = h.run_mono(period, "triangle");
        // sample[0]: phase 0.0 → trough; sample[50]: phase 0.5 → peak
        assert_within!(-1.0, samples[0], 1e-5_f32); // exact at phase boundaries; 1e-5 for f32 rounding
        assert_within!(1.0, samples[50], 1e-5_f32);
    }

    #[test]
    fn sawtooth_polyblep_smooths_transition() {
        let period = 100_usize;
        let mut h = make_osc(0.0, C0_FREQ * period as f32);
        h.tick();
        let v = h.read_mono("sawtooth");
        assert!(v > -1.0, "sawtooth at wrap transition must not output exact -1.0; got {v}");
    }

    #[test]
    fn sawtooth_non_transition_samples_match_formula() {
        let period = 100_usize;
        let mut h = make_osc(0.0, C0_FREQ * period as f32);
        h.tick(); // i=0 is the transition; skip
        for i in 1..period {
            h.tick();
            let v = h.read_mono("sawtooth");
            let phase = i as f32 / period as f32;
            let expected = 2.0 * phase - 1.0;
            // Phase increments are exact at this sample_rate; 1e-5 accounts for f32 arithmetic
            assert_within!(expected, v, 1e-5_f32);
        }
    }

    #[test]
    fn square_polyblep_at_transition_not_exactly_plus_minus_one() {
        let period = 100_usize;
        let mut h = make_osc(0.0, C0_FREQ * period as f32);
        h.tick();
        let v = h.read_mono("square");
        assert!(v > -1.0 && v < 1.0, "square at rising edge must not be exactly ±1; got {v}");

        h.run_mono(49, "square");
        h.tick();
        let v = h.read_mono("square");
        assert!(
            v > -1.0 && v < 1.0,
            "square at falling edge must not be exactly ±1; got {v}"
        );
    }

    #[test]
    fn square_duty_cycle_responds_to_pulse_width_input() {
        let period = 100_usize;
        let sample_rate = C0_FREQ * period as f32;

        // Connect only pulse_width and square.
        let mut h = ModuleHarness::build_with_env::<Oscillator>(
            params!["frequency" => 0.0_f32],
            env(sample_rate),
        );
        h.disconnect_inputs(&["voct", "fm", "phase_mod"]);
        h.disconnect_output("sine");
        h.disconnect_output("triangle");
        h.disconnect_output("sawtooth");

        // pulse_width = 1.0 → duty = 0.5 + 0.5*1.0 = 1.0, clamped to 0.99
        h.set_mono("pulse_width_cv", 1.0);
        let positive_count = h.run_mono(period, "square")
            .into_iter()
            .filter(|&v| v > 0.0)
            .count();
        assert!(
            positive_count >= 95,
            "expected ~99 positive samples with pw=1.0, got {positive_count}"
        );
    }

    #[test]
    fn disconnected_outputs_are_not_written() {
        let mut h = ModuleHarness::build_with_env::<Oscillator>(
            params!["frequency" => 4.75_f32],
            env(44100.0),
        );
        h.disconnect_all_inputs();
        h.disconnect_all_outputs();
        // Seed the pool with a sentinel; if the oscillator writes despite
        // connected=false the sentinel will change.
        h.init_pool(CableValue::Mono(99.0));
        h.tick();
        for name in &["sine", "triangle", "sawtooth", "square"] {
            assert_eq!(
                99.0_f32,
                h.read_mono(name),
                "output '{name}' was written despite being disconnected"
            );
        }
    }

    #[test]
    fn phase_mod_half_cycle_shifts_sine_output() {
        // Connect only phase_mod and sine.
        let mut h = ModuleHarness::build_with_env::<Oscillator>(
            params!["frequency" => 4.75_f32],
            env(44100.0),
        );
        h.disconnect_inputs(&["voct", "fm", "pulse_width_cv"]);
        h.disconnect_output("triangle");
        h.disconnect_output("sawtooth");
        h.disconnect_output("square");

        h.set_mono("phase_mod", 0.5);
        h.tick();
        // phase_mod shifts the raw phase (0.0) by exactly 0.5; lookup table max error ~1e-6
        assert_within!(lookup_sine(0.5), h.read_mono("sine"), 1e-6_f32);
    }

    #[test]
    fn phase_mod_disconnected_restores_normal_sine() {
        let mut h = make_osc(4.75, 44100.0);
        // make_osc disconnects all inputs; only connect sine output.
        h.disconnect_output("triangle");
        h.disconnect_output("sawtooth");
        h.disconnect_output("square");

        h.tick();
        // lookup_sine(0.0) returns exactly 0.0; 1e-6 accounts for any startup variation
        assert_within!(lookup_sine(0.0), h.read_mono("sine"), 1e-6_f32);
    }
}
