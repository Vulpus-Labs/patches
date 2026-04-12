use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort, TriggerInput,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use crate::common::approximate::lookup_sine;
use patches_dsp::xorshift64;

/// A low-frequency oscillator with six waveform outputs.
///
/// Outputs sine, triangle, saw_up, saw_down, square, and random waveforms.
/// Rate is in Hz; phase_offset shifts all waveforms by a fixed fraction of a cycle.
/// Mode controls polarity: bipolar ([-1, 1]), unipolar_positive ([0, 1]),
/// or unipolar_negative ([-1, 0]).
///
/// The `sync` input resets the phase to 0 on each rising edge (transition from <= 0 to > 0).
/// The `rate_cv` input adds an offset in Hz to the base `rate` parameter; the result is
/// clamped to [0.001, 40.0] Hz before use.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `sync` | mono | Rising-edge sync resets phase to 0 |
/// | `rate_cv` | mono | Additive rate offset in Hz |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `sine` | mono | Sine waveform |
/// | `triangle` | mono | Triangle waveform |
/// | `saw_up` | mono | Rising sawtooth waveform |
/// | `saw_down` | mono | Falling sawtooth waveform |
/// | `square` | mono | Square waveform (50% duty) |
/// | `random` | mono | Sample-and-hold random value (updates once per cycle) |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `rate` | float | 0.01 -- 20.0 | `1.0` | Oscillation rate in Hz |
/// | `phase_offset` | float | 0.0 -- 1.0 | `0.0` | Fixed phase offset as fraction of a cycle |
/// | `mode` | enum | bipolar, unipolar_positive, unipolar_negative | `bipolar` | Output polarity mode |
pub struct Lfo {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    phase: f32,
    phase_increment: f32,
    phase_offset: f32,
    mode: PolarityMode,
    rate: f32,
    prng_state: u64,
    random_value: f32,
    // Input port fields
    in_sync: TriggerInput,
    in_rate_cv: MonoInput,
    // Output port fields
    out_sine: MonoOutput,
    out_triangle: MonoOutput,
    out_saw_up: MonoOutput,
    out_saw_down: MonoOutput,
    out_square: MonoOutput,
    out_random: MonoOutput,
}

#[derive(Clone, Copy, PartialEq)]
enum PolarityMode {
    Bipolar,
    UniposPositive,
    UnipolarNegative,
}

fn apply_mode(v: f32, mode: PolarityMode) -> f32 {
    match mode {
        PolarityMode::Bipolar => v,
        PolarityMode::UniposPositive => 0.5 + 0.5 * v,
        PolarityMode::UnipolarNegative => -(0.5 + 0.5 * v),
    }
}

impl Module for Lfo {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Lfo", shape.clone())
            .mono_in("sync")
            .mono_in("rate_cv")
            .mono_out("sine")
            .mono_out("triangle")
            .mono_out("saw_up")
            .mono_out("saw_down")
            .mono_out("square")
            .mono_out("random")
            .float_param("rate", 0.01, 20.0, 1.0)
            .float_param("phase_offset", 0.0, 1.0, 0.0)
            .enum_param("mode", &["bipolar", "unipolar_positive", "unipolar_negative"], "bipolar")
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let prng_state = instance_id.as_u64() + 1; // +1 ensures non-zero (xorshift64 requires state != 0)
        Self {
            instance_id,
            descriptor,
            sample_rate: audio_environment.sample_rate,
            phase: 0.0,
            phase_increment: 1.0 / audio_environment.sample_rate,
            phase_offset: 0.0,
            mode: PolarityMode::Bipolar,
            rate: 1.0,
            prng_state,
            random_value: 0.0,
            in_sync: TriggerInput::default(),
            in_rate_cv: MonoInput::default(),
            out_sine: MonoOutput::default(),
            out_triangle: MonoOutput::default(),
            out_saw_up: MonoOutput::default(),
            out_saw_down: MonoOutput::default(),
            out_square: MonoOutput::default(),
            out_random: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("rate") {
            self.rate = *v;
            self.phase_increment = v / self.sample_rate;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("phase_offset") {
            self.phase_offset = *v;
        }
        if let Some(ParameterValue::Enum(v)) = params.get_scalar("mode") {
            self.mode = match *v {
                "bipolar" => PolarityMode::Bipolar,
                "unipolar_positive" => PolarityMode::UniposPositive,
                "unipolar_negative" => PolarityMode::UnipolarNegative,
                _ => return,
            };
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_sync = TriggerInput::from_ports(inputs, 0);
        self.in_rate_cv = MonoInput::from_ports(inputs, 1);
        self.out_sine = MonoOutput::from_ports(outputs, 0);
        self.out_triangle = MonoOutput::from_ports(outputs, 1);
        self.out_saw_up = MonoOutput::from_ports(outputs, 2);
        self.out_saw_down = MonoOutput::from_ports(outputs, 3);
        self.out_square = MonoOutput::from_ports(outputs, 4);
        self.out_random = MonoOutput::from_ports(outputs, 5);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        // Sync: rising edge resets phase before advance (standard 0.5 threshold).
        if self.in_sync.is_connected() && self.in_sync.tick(pool) {
            self.phase = 0.0;
        }

        // Rate CV: recompute increment per-sample when connected.
        let increment = if self.in_rate_cv.is_connected() {
            (self.rate + pool.read_mono(&self.in_rate_cv)).clamp(0.001, 40.0) / self.sample_rate
        } else {
            self.phase_increment
        };

        let new_phase = self.phase + increment;
        let wrapped = new_phase >= 1.0;
        self.phase = new_phase.fract();

        if wrapped {
            self.random_value = xorshift64(&mut self.prng_state);
        }

        let read_phase = (self.phase + self.phase_offset).fract();
        let mode = self.mode;

        if self.out_sine.is_connected() {
            pool.write_mono(&self.out_sine, apply_mode(lookup_sine(read_phase), mode));
        }
        if self.out_triangle.is_connected() {
            pool.write_mono(&self.out_triangle, apply_mode(1.0 - 4.0 * (read_phase - 0.5).abs(), mode));
        }
        if self.out_saw_up.is_connected() {
            pool.write_mono(&self.out_saw_up, apply_mode(2.0 * read_phase - 1.0, mode));
        }
        if self.out_saw_down.is_connected() {
            pool.write_mono(&self.out_saw_down, apply_mode(1.0 - 2.0 * read_phase, mode));
        }
        if self.out_square.is_connected() {
            let v = if read_phase < 0.5 { 1.0 } else { -1.0 };
            pool.write_mono(&self.out_square, apply_mode(v, mode));
        }
        if self.out_random.is_connected() {
            pool.write_mono(&self.out_random, apply_mode(self.random_value, mode));
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{AudioEnvironment, CableValue};
    use patches_core::test_support::{assert_within, ModuleHarness, params};

    fn env(sample_rate: f32) -> AudioEnvironment {
        AudioEnvironment { sample_rate, poly_voices: 16, periodic_update_interval: 32 }
    }

    fn make_lfo(rate: f32, sample_rate: f32) -> ModuleHarness {
        let mut h = ModuleHarness::build_with_env::<Lfo>(
            params!["rate" => rate],
            env(sample_rate),
        );
        // Most tests don't use CV inputs.
        h.disconnect_all_inputs();
        h
    }

    fn make_lfo_with_cv(rate: f32, sample_rate: f32) -> ModuleHarness {
        // All ports connected (harness default).
        ModuleHarness::build_with_env::<Lfo>(
            params!["rate" => rate],
            env(sample_rate),
        )
    }

    #[test]
    fn sine_output_consistent_across_two_cycles() {
        let rate = 1.0_f32;
        let period = 100_usize;
        let sample_rate = rate * period as f32;
        let mut h = make_lfo(rate, sample_rate);
        let cycle1 = h.run_mono(period, "sine");
        let cycle2 = h.run_mono(period, "sine");
        for (a, b) in cycle1.iter().zip(cycle2.iter()) {
            assert_within!(*a, *b, 1e-4_f32);
        }
    }

    #[test]
    fn phase_offset_shifts_sine_by_quarter_cycle() {
        let rate = 1.0_f32;
        let period = 100_usize;
        let sample_rate = rate * period as f32;

        let mut base = make_lfo(rate, sample_rate);
        let base_cycle = base.run_mono(period, "sine");

        let mut shifted = ModuleHarness::build_with_env::<Lfo>(
            params!["rate" => rate, "phase_offset" => 0.25_f32],
            env(sample_rate),
        );
        shifted.disconnect_all_inputs();
        let shifted_cycle = shifted.run_mono(period, "sine");

        let quarter = period / 4;
        for i in 0..period {
            let base_val = base_cycle[(i + quarter) % period];
            let shifted_val = shifted_cycle[i];
            assert!(
                (base_val - shifted_val).abs() < 1e-5,
                "phase_offset=0.25 mismatch at sample {i}: base[{}]={base_val}, shifted={shifted_val}",
                (i + quarter) % period,
            );
        }
    }

    #[test]
    fn unipolar_positive_maps_saw_up_to_zero_one() {
        let rate = 1.0_f32;
        let period = 100_usize;
        let sample_rate = rate * period as f32;
        let mut h = ModuleHarness::build_with_env::<Lfo>(
            params!["rate" => rate, "mode" => "unipolar_positive"],
            env(sample_rate),
        );
        h.disconnect_all_inputs();
        for v in h.run_mono(period, "saw_up") {
            assert!(v >= 0.0 && v <= 1.0, "unipolar_positive saw_up must be in [0, 1]; got {v}");
        }
    }

    #[test]
    fn random_output_holds_per_period_and_is_in_range() {
        let rate = 1.0_f32;
        let period = 100_usize;
        let sample_rate = rate * period as f32;
        let mut h = ModuleHarness::build_with_env::<Lfo>(
            params!["rate" => rate, "mode" => "unipolar_positive"],
            env(sample_rate),
        );
        h.disconnect_all_inputs();

        for _cycle in 0..3 {
            h.tick();
            let cycle_value = h.read_mono("random");
            assert!(
                cycle_value >= 0.0 && cycle_value <= 1.0,
                "random output must be in [0, 1] in unipolar_positive mode; got {cycle_value}"
            );
            for _ in 1..(period - 1) {
                h.tick();
                let v = h.read_mono("random");
                assert!(
                    (v - cycle_value).abs() < 1e-15,
                    "random output must hold within a period; changed from {cycle_value} to {v}"
                );
            }
            h.tick(); // end of period
        }
    }

    #[test]
    fn disconnected_outputs_are_not_written() {
        let mut h = make_lfo(1.0, 44100.0);
        h.disconnect_all_outputs();
        h.init_pool(CableValue::Mono(99.0));
        h.tick();
        for name in &["sine", "triangle", "saw_up", "saw_down", "square", "random"] {
            assert_eq!(
                99.0_f32,
                h.read_mono(name),
                "output '{name}' was written despite being disconnected"
            );
        }
    }

    #[test]
    fn sync_rising_edge_resets_phase_mid_cycle() {
        let rate = 1.0_f32;
        let period = 100_usize;
        let sample_rate = rate * period as f32;

        let mut h = make_lfo_with_cv(rate, sample_rate);
        h.set_mono("sync", 0.0);
        h.set_mono("rate_cv", 0.0);
        h.run_mono(25, "sine"); // advance 25 samples (quarter-cycle) with sync low

        // Rising edge: sync goes 0 → 1.
        h.set_mono("sync", 1.0);
        h.tick();
        let after_reset = h.read_mono("sine");

        // A fresh LFO at sample 1 should match.
        let mut fresh = make_lfo(rate, sample_rate);
        fresh.tick();
        let expected = fresh.read_mono("sine");

        assert!(
            (after_reset - expected).abs() < 1e-10,
            "after sync reset sine={after_reset}, expected fresh LFO sine={expected}"
        );
    }

    #[test]
    fn sync_level_does_not_retrigger() {
        let rate = 1.0_f32;
        let period = 100_usize;
        let sample_rate = rate * period as f32;

        // Trigger a rising edge (prev=0 → 1), then hold high.
        let mut h = make_lfo_with_cv(rate, sample_rate);
        h.set_mono("sync", 1.0);
        h.set_mono("rate_cv", 0.0);
        h.tick(); // rising edge
        let values = h.run_mono(25, "sine");

        // Reference: identical LFO, same sequence.
        let mut r = make_lfo_with_cv(rate, sample_rate);
        r.set_mono("sync", 1.0);
        r.set_mono("rate_cv", 0.0);
        r.tick();
        let ref_values = r.run_mono(25, "sine");

        for (i, (&v, &r)) in values.iter().zip(ref_values.iter()).enumerate() {
            assert!(
                (v - r).abs() < 1e-10,
                "sample {i}: sync level caused retrigger; got {v} vs ref {r}"
            );
        }
    }

    #[test]
    fn rate_cv_doubles_rate_halves_period() {
        let base_rate = 1.0_f32;
        let period = 100_usize;
        let sample_rate = base_rate * period as f32;

        let mut h = make_lfo_with_cv(base_rate, sample_rate);
        h.set_mono("sync", 0.0);
        h.set_mono("rate_cv", 1.0); // +1 Hz → effective 2 Hz → 50-sample period
        let cycle1 = h.run_mono(50, "sine");
        let cycle2 = h.run_mono(50, "sine");

        for (i, (a, b)) in cycle1.iter().zip(cycle2.iter()).enumerate() {
            assert!(
                (*a - *b).abs() < 1e-4,
                "rate_cv=+1 should produce 50-sample period; mismatch at sample {i}: {a} vs {b}"
            );
        }
    }

    #[test]
    fn rate_cv_large_negative_is_clamped() {
        let rate = 1.0_f32;
        let period = 100_usize;
        let sample_rate = rate * period as f32;

        let mut h = make_lfo_with_cv(rate, sample_rate);
        h.set_mono("sync", 0.0);
        h.set_mono("rate_cv", -1000.0);
        h.tick();
        let first = h.read_mono("sine");
        h.tick();
        let second = h.read_mono("sine");

        assert!(
            (second - first).abs() > 1e-10,
            "rate_cv=-1000 clamped to minimum should still advance phase; got first={first}, second={second}"
        );
    }
}
