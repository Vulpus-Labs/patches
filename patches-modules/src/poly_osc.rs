use patches_core::{
    AudioEnvironment, BoundedRandomWalk, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoInput, OutputPort, PolyInput, PolyOutput,
    GLOBAL_DRIFT, HALF_SEMITONE_VOCT, OSCILLATOR_DRIFT_STEP,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use crate::oscillator::OscFmType;
use crate::common::approximate::lookup_sine;
use patches_dsp::polyblep;
use crate::common::frequency::{C0_FREQ, FMMode, PolyFrequencyConverter, PolyFrequencyChangeTracker};
use crate::common::phase_accumulator::PolyPhaseAccumulator;

/// Number of samples between drift state updates for the per-voice drift random walks.
const DRIFT_PERIOD: u8 = 64;

/// Polyphonic multi-waveform oscillator.
///
/// One phase accumulator per voice (up to `poly_voices` from [`AudioEnvironment`]).
/// All voices are driven by the `voct` poly input; channel `i` controls voice `i`.
/// Outputs four poly waveforms; only connected outputs are computed each sample.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `voct` | poly | V/oct pitch CV per voice |
/// | `fm` | poly | Frequency modulation input per voice |
/// | `pulse_width_cv` | poly | Pulse width modulation for the square output per voice |
/// | `phase_mod` | poly | Phase modulation offset applied to all waveforms per voice |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `sine` | poly | Sine waveform |
/// | `triangle` | poly | Triangle waveform |
/// | `sawtooth` | poly | Sawtooth waveform (PolyBLEP anti-aliased) |
/// | `square` | poly | Square waveform (PolyBLEP anti-aliased, PWM via `pulse_width_cv`) |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `frequency` | float | -4.0 -- 12.0 | `0.0` | Base pitch as V/oct offset from C0 |
/// | `fm_type` | enum | linear, logarithmic | `linear` | FM modulation mode |
/// | `drift` | float | 0.0 -- 1.0 | `0.0` | Pitch drift amount (per-voice random walk + global drift) |
pub struct PolyOsc {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    phase_acc: PolyPhaseAccumulator,
    freq_converter: PolyFrequencyConverter,
    freq_tracker: PolyFrequencyChangeTracker,
    // Port fields
    in_voct: PolyInput,
    in_fm: PolyInput,
    in_pulse_width: PolyInput,
    in_phase_mod: PolyInput,
    /// Fixed input pointing at the engine-level global drift backplane slot.
    in_global_drift: MonoInput,
    out_sine: PolyOutput,
    out_triangle: PolyOutput,
    out_sawtooth: PolyOutput,
    out_square: PolyOutput,
    // Drift state
    /// `drift` parameter value in [0.0, 1.0]. Zero disables drift entirely.
    drift: f32,
    /// Independent random walk per voice for local pitch drift.
    drift_walks: [BoundedRandomWalk; 16],
    /// Counts samples since last drift update; resets to 0 every `DRIFT_PERIOD`.
    drift_counter: u8,
    /// Per-voice V/OCT offset added during frequency calculation.
    drift_voct_offsets: [f32; 16],
}

impl Module for PolyOsc {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("PolyOsc", shape.clone())
            .poly_in("voct")
            .poly_in("fm")
            .poly_in("pulse_width_cv")
            .poly_in("phase_mod")
            .poly_out("sine")
            .poly_out("triangle")
            .poly_out("sawtooth")
            .poly_out("square")
            .float_param("frequency", -4.0, 12.0, 0.0)
            .enum_param("fm_type", OscFmType::VARIANTS, "linear")
            .float_param("drift", 0.0, 1.0, 0.0)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        // Derive non-zero, per-voice seeds from instance_id so each voice drifts independently.
        let base_seed = instance_id.as_u64().wrapping_add(1) as u32;
        let drift_walks = std::array::from_fn(|i| {
            BoundedRandomWalk::new(base_seed.wrapping_add(i as u32), OSCILLATOR_DRIFT_STEP)
        });
        Self {
            instance_id,
            descriptor,
            phase_acc: PolyPhaseAccumulator::new(),
            freq_converter: PolyFrequencyConverter::new(audio_environment.sample_rate),
            freq_tracker: PolyFrequencyChangeTracker::new(C0_FREQ),
            in_voct: PolyInput::default(),
            in_fm: PolyInput::default(),
            in_pulse_width: PolyInput::default(),
            in_phase_mod: PolyInput::default(),
            in_global_drift: MonoInput { cable_idx: GLOBAL_DRIFT, scale: 1.0, connected: true },
            out_sine: PolyOutput::default(),
            out_triangle: PolyOutput::default(),
            out_sawtooth: PolyOutput::default(),
            out_square: PolyOutput::default(),
            drift: 0.0,
            drift_walks,
            drift_counter: 0,
            drift_voct_offsets: [0.0; 16],
        }
    }

    fn update_validated_parameters(&mut self, params: &ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("frequency") {
            self.freq_tracker.set_voct_offset(*v);
            let inc = self.freq_converter.to_increment(self.freq_tracker.base_frequency());
            self.phase_acc.set_all_increments(inc);
        }
        if let Some(&ParameterValue::Enum(v)) = params.get_scalar("fm_type") {
            if let Ok(t) = OscFmType::try_from(v) {
                let fm_mode = match t {
                    OscFmType::Linear => FMMode::Linear,
                    OscFmType::Logarithmic => FMMode::Exponential,
                };
                self.freq_tracker.set_fm_mode(fm_mode);
            }
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("drift") {
            self.drift = *v;
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_voct        = PolyInput::from_ports(inputs, 0);
        self.in_fm          = PolyInput::from_ports(inputs, 1);
        self.in_pulse_width = PolyInput::from_ports(inputs, 2);
        self.in_phase_mod   = PolyInput::from_ports(inputs, 3);
        self.out_sine     = PolyOutput::from_ports(outputs, 0);
        self.out_triangle = PolyOutput::from_ports(outputs, 1);
        self.out_sawtooth = PolyOutput::from_ports(outputs, 2);
        self.out_square   = PolyOutput::from_ports(outputs, 3);

        self.freq_tracker.voct_modulating = self.in_voct.is_connected();
        self.freq_tracker.fm_modulating   = self.in_fm.is_connected();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let voct = if self.in_voct.is_connected() {
            pool.read_poly(&self.in_voct)
        } else {
            [0.0; 16]
        };
        let fm = if self.in_fm.is_connected() {
            pool.read_poly(&self.in_fm)
        } else {
            [0.0; 16]
        };
        let phase_mod = if self.in_phase_mod.is_connected() {
            pool.read_poly(&self.in_phase_mod)
        } else {
            [0.0; 16]
        };

        // Drift: every DRIFT_PERIOD samples, advance each voice's independent walk
        // and sample the engine-level global drift, then update per-voice offsets.
        let force_recalc = if self.drift > 0.0 {
            self.drift_counter = self.drift_counter.wrapping_add(1);
            if self.drift_counter >= DRIFT_PERIOD {
                self.drift_counter = 0;
                let global_val = pool.read_mono(&self.in_global_drift);
                // Each voice: global component (shared) + local component (independent).
                // Each in [-1, 1]; scale so combined max = ±HALF_SEMITONE_VOCT.
                let scale = HALF_SEMITONE_VOCT * 0.5 * self.drift;
                for i in 0..16 {
                    let local_val = self.drift_walks[i].advance();
                    self.drift_voct_offsets[i] = (global_val + local_val) * scale;
                }
                true
            } else {
                false
            }
        } else {
            false
        };

        // Update per-voice increments when modulating or when drift forces a recalc.
        if self.freq_tracker.is_modulating() {
            for i in 0..16 {
                let freq = self.freq_tracker.compute_modulated(i, voct[i] + self.drift_voct_offsets[i], fm[i]);
                self.phase_acc.set_increment(i, self.freq_converter.to_increment(freq));
            }
        } else if force_recalc {
            // No voct/fm modulation but drift changed: recompute from base frequency per voice.
            let base_freq = self.freq_tracker.base_frequency();
            for i in 0..16 {
                let freq = base_freq * self.drift_voct_offsets[i].exp2();
                self.phase_acc.set_increment(i, self.freq_converter.to_increment(freq));
            }
        }

        let do_sine = self.out_sine.is_connected();
        let do_tri  = self.out_triangle.is_connected();
        let do_saw  = self.out_sawtooth.is_connected();
        let do_sq   = self.out_square.is_connected();

        if !do_sine && !do_tri && !do_saw && !do_sq {
            // Advance phases even when no outputs connected, so pitch stays coherent.
            self.phase_acc.advance_all();
            return;
        }

        let pw_connected = self.in_pulse_width.is_connected();
        let pulse_widths = if pw_connected {
            pool.read_poly(&self.in_pulse_width)
        } else {
            [0.0; 16]
        };

        let phase_mod_connected = self.in_phase_mod.is_connected();

        let mut sine_out = [0.0f32; 16];
        let mut tri_out  = [0.0f32; 16];
        let mut saw_out  = [0.0f32; 16];
        let mut sq_out   = [0.0f32; 16];

        for i in 0..16 {
            let raw_phase = self.phase_acc.phases[i];
            // phase_mod is in [-1, 1]; raw_phase is in [0, 1), so sum is in [-1, 2).
            // `sum - sum.floor()` maps that range correctly to [0, 1) and vectorises
            // (floor → frintm on aarch64 NEON), unlike rem_euclid.
            let phase = if phase_mod_connected {
                let sum = raw_phase + phase_mod[i].clamp(-1.0, 1.0);
                sum - sum.floor()
            } else {
                raw_phase
            };
            let dt = self.phase_acc.phase_increments[i];

            if do_sine { sine_out[i] = lookup_sine(phase); }
            if do_tri  { tri_out[i]  = 1.0 - 4.0 * (phase - 0.5).abs(); }
            if do_saw  { saw_out[i]  = (2.0 * phase - 1.0) - polyblep(phase, dt); }
            if do_sq {
                let duty = if pw_connected {
                    (0.5 + 0.5 * pulse_widths[i]).clamp(0.01, 0.99)
                } else {
                    0.5
                };
                let raw = if phase < duty { 1.0 } else { -1.0 };
                let blep = polyblep(phase, dt) - polyblep((phase - duty).rem_euclid(1.0), dt);
                sq_out[i] = raw + blep;
            }
        }

        self.phase_acc.advance_all();

        if do_sine { pool.write_poly(&self.out_sine,     sine_out); }
        if do_tri  { pool.write_poly(&self.out_triangle, tri_out);  }
        if do_saw  { pool.write_poly(&self.out_sawtooth, saw_out);  }
        if do_sq   { pool.write_poly(&self.out_square,   sq_out);   }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::approximate::lookup_sine;
    use patches_core::{AudioEnvironment, CableValue};
    use patches_core::test_support::{assert_within, ModuleHarness, params};

    fn env(sample_rate: f32, voices: usize) -> AudioEnvironment {
        AudioEnvironment { sample_rate, poly_voices: voices, periodic_update_interval: 32, hosted: false }
    }

    /// Build a harness with all CV inputs disconnected. Most tests don't need modulation.
    fn make_poly_osc(sample_rate: f32, voices: usize) -> ModuleHarness {
        let mut h = ModuleHarness::build_with_env::<PolyOsc>(
            params!["frequency" => 0.0_f32],
            env(sample_rate, voices),
        );
        h.disconnect_all_inputs();
        h
    }

    #[test]
    fn disconnected_outputs_are_not_written() {
        let mut h = make_poly_osc(44100.0, 4);
        h.disconnect_all_outputs();
        // Seed pool with sentinel; any write by the oscillator will overwrite it.
        h.init_pool(CableValue::Poly([99.0; 16]));
        h.tick();
        for name in &["sine", "triangle", "sawtooth", "square"] {
            let out = h.read_poly(name);
            for (i, &v) in out.iter().take(4).enumerate() {
                assert_eq!(99.0_f32, v, "output '{name}' voice {i} was written despite being disconnected");
            }
        }
    }

    #[test]
    fn sine_output_correct_shape() {
        // At sample_rate = C0*100, each tick advances phase by 1/100.
        // The 26th tick processes phase 0.25 (quarter-period), where sine peaks.
        // lookup table max error ~1e-4; 1e-3 gives headroom for f32 phase accumulation.
        let period = 100_usize;
        let mut h = make_poly_osc(C0_FREQ * period as f32, 1);
        let samples = h.run_poly(26, "sine");
        let v = samples.last().unwrap()[0];
        assert_within!(lookup_sine(0.25), v, 1e-3_f32);
    }

    #[test]
    fn triangle_output_correct_shape() {
        // triangle = 1.0 - 4.0 * (phase - 0.5).abs()
        // phase 0.0 → trough = -1.0; phase 0.5 → peak = +1.0.
        // sample[0]: phase 0.0; sample[50]: phase 0.5.
        let period = 100_usize;
        let mut h = make_poly_osc(C0_FREQ * period as f32, 1);
        let samples = h.run_poly(period, "triangle");
        // Exact at phase boundaries; 1e-5 accounts for f32 rounding
        assert_within!(-1.0, samples[0][0], 1e-5_f32);
        assert_within!(1.0, samples[50][0], 1e-5_f32);
    }

    #[test]
    fn square_polyblep_edges_smoothed() {
        // PolyBLEP correction ensures the square wave is never exactly ±1.0 at transitions.
        let period = 100_usize;
        let mut h = make_poly_osc(C0_FREQ * period as f32, 1);
        h.disconnect_output("sine");
        h.disconnect_output("triangle");
        h.disconnect_output("sawtooth");
        // First tick: rising edge (phase 0 → dt). PolyBLEP corrects the discontinuity.
        h.tick();
        let v = h.read_poly("square")[0];
        assert!(v > -1.0 && v < 1.0, "square at rising edge must not be exactly ±1; got {v}");
        // Advance to the falling edge (~50 samples into the period).
        h.run_poly(49, "square");
        h.tick();
        let v = h.read_poly("square")[0];
        assert!(v > -1.0 && v < 1.0, "square at falling edge must not be exactly ±1; got {v}");
    }

    #[test]
    fn square_duty_cycle_responds_to_pulse_width_input() {
        let period = 100_usize;
        let sample_rate = C0_FREQ * period as f32;

        // Connect only pulse_width_cv and square for voice 0.
        let mut h = ModuleHarness::build_with_env::<PolyOsc>(
            params!["frequency" => 0.0_f32],
            env(sample_rate, 1),
        );
        h.disconnect_input("voct");
        h.disconnect_input("fm");
        h.disconnect_input("phase_mod");
        h.disconnect_output("sine");
        h.disconnect_output("triangle");
        h.disconnect_output("sawtooth");

        // pulse_width = 1.0 → duty = 0.5 + 0.5*1.0 = 1.0, clamped to 0.99
        let mut pw = [0.0f32; 16];
        pw[0] = 1.0;
        h.set_poly("pulse_width_cv", pw);

        let positive_count = h.run_poly(period, "square")
            .into_iter()
            .filter(|arr| arr[0] > 0.0)
            .count();
        assert!(
            positive_count >= 95,
            "expected ~99 positive samples for voice 0 with pw=1.0; got {positive_count}"
        );
    }

    #[test]
    fn phase_mod_half_cycle_shifts_sine_output() {
        // Connect only phase_mod and sine for voice 0.
        let mut h = ModuleHarness::build_with_env::<PolyOsc>(
            params!["frequency" => 4.75_f32],
            env(44100.0, 1),
        );
        h.disconnect_input("voct");
        h.disconnect_input("fm");
        h.disconnect_input("pulse_width_cv");
        h.disconnect_output("triangle");
        h.disconnect_output("sawtooth");
        h.disconnect_output("square");

        let mut pm = [0.0f32; 16];
        pm[0] = 0.5;
        h.set_poly("phase_mod", pm);
        h.tick();
        // phase_mod shifts the raw phase (0.0) by exactly 0.5; lookup table max error ~1e-6
        assert_within!(lookup_sine(0.5), h.read_poly("sine")[0], 1e-6_f32);
    }

    #[test]
    fn voct_input_drives_independent_phases_per_voice() {
        // At sample_rate = C0 * 100, one cycle of voice 0 (voct=0) takes 100 samples.
        // Voice 1 with voct=1 (one octave up) runs at 2× and completes a cycle in 50 samples.
        // After 25 samples: voice 0 is at phase 0.25 (sine ≈ +1), voice 1 at phase 0.50 (sine ≈ 0).
        let period = 100_usize;
        let sample_rate = C0_FREQ * period as f32;
        let mut h = ModuleHarness::build_with_env::<PolyOsc>(
            params!["frequency" => 0.0_f32],
            env(sample_rate, 2),
        );
        h.disconnect_input("fm");
        h.disconnect_input("pulse_width_cv");
        h.disconnect_input("phase_mod");
        h.disconnect_output("triangle");
        h.disconnect_output("sawtooth");
        h.disconnect_output("square");

        let mut voct = [0.0f32; 16];
        voct[1] = 1.0; // voice 1: one octave up
        h.set_poly("voct", voct);

        let sines = h.run_poly(25, "sine");
        let last = *sines.last().unwrap();
        // Voice 0 at phase 0.24 → sine near +1 (phase 0.25 peaks)
        assert!(last[0] > 0.9, "voice 0 at 0.25 cycle, sine should be near +1; got {}", last[0]);
        // Voice 1 at phase 0.48 → sine near 0 (phase 0.5 is zero-crossing)
        // lookup table max error ~1e-4; 0.15 tolerance for phase slightly before 0.5
        assert_within!(0.0, last[1], 0.15_f32);
    }
}
