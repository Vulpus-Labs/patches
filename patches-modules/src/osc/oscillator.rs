use patches_core::{
    params_enum,
    AudioEnvironment, BoundedRandomWalk, CablePool, CountAxis, InputPort, InstanceId, Module,
    ModuleDescriptor, ModuleDescriptorTemplate, MonoInput, MonoOutput, OutputPort,
    ParameterKind, ParameterTemplate, PortTemplate, GLOBAL_DRIFT, HALF_SEMITONE_VOCT,
    OSCILLATOR_DRIFT_STEP,
};
use patches_core::{StructuralParams, BuildError};
use patches_core::cables::TriggerInput;
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
/// | `sync` | trigger | Sub-sample hard-sync (ADR 0047): on event at `frac`, phase resets and saw/square apply PolyBLEP scaled by pre→post jump |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `sine` | mono | Sine waveform |
/// | `triangle` | mono | Triangle waveform |
/// | `sawtooth` | mono | Sawtooth waveform (PolyBLEP anti-aliased) |
/// | `square` | mono | Square waveform (PolyBLEP anti-aliased, PWM via `pulse_width_cv`) |
/// | `reset_out` | trigger | Sub-sample fractional position of each phase wrap (ADR 0047) |
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
    in_sync: TriggerInput,
    /// Fixed input pointing at the engine-level global drift backplane slot.
    in_global_drift: MonoInput,
    // Output port fields
    out_sine: MonoOutput,
    out_triangle: MonoOutput,
    out_sawtooth: MonoOutput,
    out_square: MonoOutput,
    out_reset: MonoOutput,
    // Drift state
    /// `drift` parameter value in [0.0, 1.0]. Zero disables drift entirely.
    drift: f32,
    /// Per-instance random walk for local pitch drift.
    drift_walk: BoundedRandomWalk,
    /// Counts samples since last drift update; resets to 0 every `DRIFT_PERIOD`.
    drift_counter: u8,
    /// Current V/OCT offset added to the voct input each frequency calculation.
    drift_voct_offset: f32,
    /// Deferred sync state (ticket 0956, mirrors 0955). A sub-sample sync emits
    /// the *pre* value on the sync tick and the *post* value on the next tick
    /// (start-of-sample convention), so the post value is never duplicated.
    /// `sync_pending` flags that the previous tick was a sync; the `pending_*`
    /// fields carry the trailing half of the 2-point polyBLEP, applied on the
    /// post tick in place of the natural wrap correction.
    sync_pending: bool,
    pending_saw_blep: f32,
    pending_sq_blep: f32,
}

impl Module for Oscillator {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "Osc",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[
                PortTemplate::mono("voct"),
                PortTemplate::mono("fm"),
                PortTemplate::mono("pulse_width_cv"),
                PortTemplate::mono("phase_mod"),
                PortTemplate::trigger("sync"),
            ],
            per_axis_inputs: &[],
            global_outputs: &[
                PortTemplate::mono("sine"),
                PortTemplate::mono("triangle"),
                PortTemplate::mono("sawtooth"),
                PortTemplate::mono("square"),
                PortTemplate::trigger("reset_out"),
            ],
            per_axis_outputs: &[],
            realtime_params: &[
                ParameterTemplate {
                    name: params::frequency.as_str(),
                    kind: ParameterKind::Float { min: -4.0, max: 12.0, default: 0.0 },
                },
                ParameterTemplate {
                    name: params::fm_type.as_str(),
                    kind: ParameterKind::Enum {
                        variants: OscFmType::VARIANTS,
                        default: "linear",
                    },
                },
                ParameterTemplate {
                    name: params::drift.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.0 },
                },
            ],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, _structural: &StructuralParams) -> Result<Self, BuildError> { Ok({
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
            in_sync: TriggerInput::default(),
            in_global_drift: MonoInput::backplane(GLOBAL_DRIFT),
            out_sine: MonoOutput::default(),
            out_triangle: MonoOutput::default(),
            out_sawtooth: MonoOutput::default(),
            out_square: MonoOutput::default(),
            out_reset: MonoOutput::default(),
            drift: 0.0,
            drift_walk: BoundedRandomWalk::new(seed, OSCILLATOR_DRIFT_STEP),
            drift_counter: 0,
            drift_voct_offset: 0.0,
            sync_pending: false,
            pending_saw_blep: 0.0,
            pending_sq_blep: 0.0,
        }
    })}

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
        self.in_sync = TriggerInput::from_ports(inputs, 4);
        self.out_sine = outputs[0].expect_mono();
        self.out_triangle = outputs[1].expect_mono();
        self.out_sawtooth = outputs[2].expect_mono();
        self.out_square = outputs[3].expect_mono();
        self.out_reset = outputs[4].expect_trigger();

        self.freq_tracker.voct_modulating = self.in_voct.is_connected();
        self.freq_tracker.fm_modulating = self.in_fm.is_connected();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let sync = if self.in_sync.is_connected() { self.in_sync.tick(pool) } else { None };
        let phase_mod = if self.in_phase_mod.is_connected() {
            pool.read_mono(&self.in_phase_mod)
        } else {
            0.0
        };
        let duty = if self.in_pulse_width.is_connected() {
            (0.5 + 0.5 * pool.read_mono(&self.in_pulse_width)).clamp(0.01, 0.99)
        } else {
            0.5
        };

        let dt = self.phase_acc.phase_increment;
        let mut wrap_frac = 0.0_f32;
        let pending = self.sync_pending;

        if let Some(frac) = sync {
            let frac = frac.clamp(f32::MIN_POSITIVE, 1.0);

            // Start-of-sample value: emit what the oscillator already holds (the
            // pre value, or a deferred post in the rapid-sync case) — never the
            // post of *this* sync. The post is deferred to the next tick so it
            // is emitted exactly once (ticket 0956).
            let cur = self.phase_acc.phase;
            let read = (cur + phase_mod).rem_euclid(1.0);

            // Value at the sync instant — sizes the discontinuity only.
            let mut reset_raw = cur + frac * dt;
            if reset_raw >= 1.0 { reset_raw -= 1.0; }
            let reset_read = (reset_raw + phase_mod).rem_euclid(1.0);

            // Reset; the post value is emitted on the next tick.
            self.phase_acc.sync_reset(frac);
            let post_raw = self.phase_acc.phase;
            let post_read = (post_raw + phase_mod).rem_euclid(1.0);

            // 2-point polyBLEP: leading half here (distance `frac` before the
            // jump), trailing half deferred. Correction is `-basis * 0.5 * delta`,
            // matching the free path's `value - polyblep(...)` sign.
            let before = polyblep(1.0 - frac * dt, dt);
            let after = polyblep(post_raw, dt);

            if self.out_sine.is_connected() {
                pool.write_mono(&self.out_sine, lookup_sine(read));
            }
            if self.out_triangle.is_connected() {
                pool.write_mono(&self.out_triangle, 1.0 - 4.0 * (read - 0.5).abs());
            }
            if self.out_sawtooth.is_connected() {
                let delta = (2.0 * reset_read - 1.0) - (2.0 * post_read - 1.0);
                let wrap = if pending { self.pending_saw_blep } else { -polyblep(read, dt) };
                pool.write_mono(&self.out_sawtooth, (2.0 * read - 1.0) + wrap - before * 0.5 * delta);
                self.pending_saw_blep = -after * 0.5 * delta;
            }
            if self.out_square.is_connected() {
                let pre = if reset_read < duty { 1.0 } else { -1.0 };
                let post = if post_read < duty { 1.0 } else { -1.0 };
                let delta = pre - post;
                let raw = if read < duty { 1.0 } else { -1.0 };
                let wrap = if pending { self.pending_sq_blep } else { polyblep(read, dt) };
                let duty_edge = polyblep((read - duty).rem_euclid(1.0), dt);
                pool.write_mono(&self.out_square, raw + wrap - duty_edge - before * 0.5 * delta);
                self.pending_sq_blep = -after * 0.5 * delta;
            }
            self.sync_pending = true;
        } else {
            let phase = (self.phase_acc.phase + phase_mod).rem_euclid(1.0);

            if self.out_sine.is_connected() {
                pool.write_mono(&self.out_sine, lookup_sine(phase));
            }
            if self.out_triangle.is_connected() {
                pool.write_mono(&self.out_triangle, 1.0 - 4.0 * (phase - 0.5).abs());
            }
            if self.out_sawtooth.is_connected() {
                // On the tick after a sync this is the post sample: apply the
                // deferred trailing polyBLEP in place of the natural wrap term.
                let wrap = if pending { self.pending_saw_blep } else { -polyblep(phase, dt) };
                pool.write_mono(&self.out_sawtooth, (2.0 * phase - 1.0) + wrap);
            }
            if self.out_square.is_connected() {
                let raw = if phase < duty { 1.0 } else { -1.0 };
                let wrap = if pending { self.pending_sq_blep } else { polyblep(phase, dt) };
                let duty_edge = polyblep((phase - duty).rem_euclid(1.0), dt);
                pool.write_mono(&self.out_square, raw + wrap - duty_edge);
            }
            self.sync_pending = false;
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
        if sync.is_none() {
            wrap_frac = self.phase_acc.advance_wrap_frac();
        }
        if self.out_reset.is_connected() {
            pool.write_mono(&self.out_reset, wrap_frac);
        }
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

    /// PolyBLEP must round off discontinuities so transition samples never
    /// hit ±1.0 exactly. Sawtooth wraps at phase 0; square has rising and
    /// falling edges at phase 0 and 0.5 within one period.
    #[test]
    fn polyblep_smooths_waveform_transitions() {
        let period = 100_usize;
        let cases: &[(&str, &[usize])] = &[
            ("sawtooth", &[0]),
            ("square",   &[0, 50]),
        ];
        for &(port, transitions) in cases {
            let mut h = make_osc(0.0, C0_FREQ * period as f32);
            for i in 0..period {
                h.tick();
                let v = h.read_mono(port);
                if transitions.contains(&i) {
                    assert!(
                        v > -1.0 && v < 1.0,
                        "{port} at transition i={i} must not be exactly ±1; got {v}"
                    );
                }
            }
        }
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
        h.init_pool(CableValue::mono(99.0));
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

    // ── reset_out / sync (0636, ADR 0047) ────────────────────────────────

    #[test]
    fn reset_out_emits_wrap_frac() {
        let sr = 48_000.0_f32;
        let voct = 7.0_f32; // ~C7, a few thousand Hz → many wraps in 2000 samples
        let mut h = ModuleHarness::build_with_env::<Oscillator>(
            params!["frequency" => voct],
            env(sr),
        );
        h.disconnect_all_inputs();
        let n = 2000_usize;
        let reset = h.run_mono(n, "reset_out");
        let mut wraps = 0usize;
        for &e in &reset {
            if e > 0.0 {
                assert!(e <= 1.0, "frac out of range: {e}");
                wraps += 1;
            }
        }
        assert!(wraps >= 3, "expected ≥3 wraps in {n} samples; got {wraps}");
    }

    #[test]
    fn sync_resets_saw_to_post_advance() {
        for &frac in &[0.001_f32, 0.5, 0.999] {
            let sr = 48_000.0_f32;
            let freq = 200.0_f32;
            let voct = (freq / C0_FREQ).log2();
            let mut h = ModuleHarness::build_with_env::<Oscillator>(
                params!["frequency" => voct],
                env(sr),
            );
            h.disconnect_all_inputs();
            h.set_mono("sync", 0.0);
            let _ = h.run_mono(10, "sawtooth");
            h.set_mono("sync", frac);
            h.tick();
            h.set_mono("sync", 0.0);
            let y = h.read_mono("sawtooth");
            let dt = freq / sr;
            let expected = 2.0 * (1.0 - frac) * dt - 1.0;
            assert!(
                (y - expected).abs() < 0.6,
                "sync frac={frac}: saw {y} not near post-reset {expected}"
            );
        }
    }

    #[test]
    fn sync_all_waveforms_finite() {
        let mut h = ModuleHarness::build_with_env::<Oscillator>(
            params!["frequency" => 4.0_f32],
            env(48_000.0),
        );
        h.disconnect_all_inputs();
        h.set_mono("sync", 0.0);
        h.set_mono("pulse_width_cv", 0.3);
        let _ = h.run_mono(16, "sawtooth");
        for i in 0..128 {
            let frac = if i % 3 == 0 { 0.25 + (i as f32) * 0.001 } else { 0.0 };
            h.set_mono("sync", frac.min(0.99));
            h.tick();
            for n in ["sine", "triangle", "sawtooth", "square"] {
                let v = h.read_mono(n);
                assert!(v.is_finite(), "non-finite {n} at i={i}: {v}");
            }
        }
    }

    // ── 0956: post-reset duplicate / aliasing (mirrors 0955) ─────────────

    /// A sub-sample sync must not emit the post-reset value on two consecutive
    /// samples (exact duplicate on sine/triangle pre-fix).
    #[test]
    fn sync_does_not_duplicate_post_reset_sample() {
        let sr = 48_000.0_f32;
        let voct = (300.0_f32 / C0_FREQ).log2();
        let mut h = ModuleHarness::build_with_env::<Oscillator>(
            params!["frequency" => voct],
            env(sr),
        );
        h.disconnect_all_inputs();
        h.disconnect_output("triangle");
        h.disconnect_output("square");

        let mut sine = Vec::new();
        let mut saw = Vec::new();
        h.set_mono("sync", 0.0);
        for _ in 0..6 {
            h.tick();
            sine.push(h.read_mono("sine"));
            saw.push(h.read_mono("sawtooth"));
        }
        h.set_mono("sync", 0.37);
        h.tick();
        sine.push(h.read_mono("sine"));
        saw.push(h.read_mono("sawtooth"));
        h.set_mono("sync", 0.0);
        for _ in 0..4 {
            h.tick();
            sine.push(h.read_mono("sine"));
            saw.push(h.read_mono("sawtooth"));
        }

        for w in sine.windows(2) {
            assert_ne!(w[0].to_bits(), w[1].to_bits(),
                "sine: consecutive samples bit-equal — post-reset duplicate (0956): {sine:?}");
        }
        for w in saw.windows(2) {
            assert_ne!(w[0].to_bits(), w[1].to_bits(),
                "saw: consecutive samples bit-equal — post-reset duplicate (0956): {saw:?}");
        }
    }

    fn sync_schedule(master_dt: f32, n: usize) -> Vec<f32> {
        let mut ph = 0.0f32;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let next = ph + master_dt;
            if next >= 1.0 {
                ph = next - 1.0;
                out.push((1.0 - ph / master_dt).clamp(f32::MIN_POSITIVE, 1.0));
            } else {
                ph = next;
                out.push(0.0);
            }
        }
        out
    }

    /// Naive (un-BLEP'd) hard-synced saw on the same deferred start-of-sample
    /// trajectory as the module — the clean reset reference.
    fn naive_synced_saw(slave_dt: f32, sched: &[f32]) -> Vec<f32> {
        let mut phase = 0.0f32;
        let mut out = Vec::with_capacity(sched.len());
        for &s in sched {
            out.push(2.0 * phase - 1.0);
            if s > 0.0 {
                phase = (1.0 - s) * slave_dt;
            } else {
                let next = phase + slave_dt;
                phase = if next >= 1.0 { next - 1.0 } else { next };
            }
        }
        out
    }

    fn high_band(x: &[f32]) -> f64 {
        let n = x.len();
        let fft = patches_dsp::RealPackedFft::new(n);
        let mut buf = x.to_vec();
        fft.forward(&mut buf);
        let half = n / 2;
        let mut s = 0.0f64;
        for k in (half * 3) / 4..half {
            let re = buf[2 * k] as f64;
            let im = buf[2 * k + 1] as f64;
            s += (re * re + im * im).sqrt();
        }
        s
    }

    /// The 2-point polyBLEP'd synced saw must alias substantially less than the
    /// clean (un-BLEP'd) reset reference. Catches a returning duplicate or a
    /// residual sign error (which would raise aliasing above the naive reset).
    #[test]
    fn sync_aliasing_below_clean_reset_reference() {
        const SR: f32 = 48_000.0;
        const WARMUP: usize = 4096;
        const N: usize = 4096;
        let f_master = 468.75_f32;
        let f_slave = f_master * 2.6;
        let master_dt = f_master / SR;
        let slave_dt = f_slave / SR;
        let voct = (f_slave / C0_FREQ).log2();

        let sched = sync_schedule(master_dt, WARMUP + N);

        let mut h = ModuleHarness::build_with_env::<Oscillator>(
            params!["frequency" => voct],
            env(SR),
        );
        h.disconnect_all_inputs();
        h.disconnect_output("sine");
        h.disconnect_output("triangle");
        h.disconnect_output("square");

        let mut blep = Vec::with_capacity(WARMUP + N);
        for &s in &sched {
            h.set_mono("sync", s);
            h.tick();
            blep.push(h.read_mono("sawtooth"));
        }
        let naive = naive_synced_saw(slave_dt, &sched);

        let blep_hi = high_band(&blep[WARMUP..]);
        let naive_hi = high_band(&naive[WARMUP..]);

        assert!(
            blep_hi * 1.4 < naive_hi,
            "polyBLEP synced saw aliasing ({blep_hi:.4}) not >=1.4x below clean reset ({naive_hi:.4})"
        );
    }
}
