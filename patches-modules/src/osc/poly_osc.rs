use patches_core::{
    AudioEnvironment, BoundedRandomWalk, CablePool, CountAxis, InputPort, InstanceId, Module,
    ModuleDescriptor, ModuleDescriptorTemplate, MonoInput, OutputPort, ParameterKind,
    ParameterTemplate, PolyInput, PolyOutput, PortTemplate,
    GLOBAL_DRIFT, HALF_SEMITONE_VOCT, OSCILLATOR_DRIFT_STEP,
};
use patches_core::{StructuralParams, BuildError};
use patches_core::cables::PolyTriggerInput;
use patches_core::module_params;
use patches_core::param_frame::ParamView;
use super::oscillator::OscFmType;

module_params! {
    PolyOsc {
        frequency: Float,
        fm_type:   Enum<OscFmType>,
        drift:     Float,
    }
}
use crate::common::approximate::lookup_sine;
use patches_dsp::{polyblep, sync_blep_residual};
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
/// | `sync` | poly_trigger | Per-voice sub-sample hard-sync (ADR 0047) |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `sine` | poly | Sine waveform |
/// | `triangle` | poly | Triangle waveform |
/// | `sawtooth` | poly | Sawtooth waveform (PolyBLEP anti-aliased) |
/// | `square` | poly | Square waveform (PolyBLEP anti-aliased, PWM via `pulse_width_cv`) |
/// | `reset_out` | poly_trigger | Per-voice sub-sample fractional position of each phase wrap (ADR 0047) |
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
    in_sync: PolyTriggerInput,
    /// Fixed input pointing at the engine-level global drift backplane slot.
    in_global_drift: MonoInput,
    out_sine: PolyOutput,
    out_triangle: PolyOutput,
    out_sawtooth: PolyOutput,
    out_square: PolyOutput,
    out_reset: PolyOutput,
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
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "PolyOsc",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[
                PortTemplate::poly("voct"),
                PortTemplate::poly("fm"),
                PortTemplate::poly("pulse_width_cv"),
                PortTemplate::poly("phase_mod"),
                PortTemplate::poly_trigger("sync"),
            ],
            per_axis_inputs: &[],
            global_outputs: &[
                PortTemplate::poly("sine"),
                PortTemplate::poly("triangle"),
                PortTemplate::poly("sawtooth"),
                PortTemplate::poly("square"),
                PortTemplate::poly_trigger("reset_out"),
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
            in_sync: PolyTriggerInput::default(),
            in_global_drift: MonoInput::backplane(GLOBAL_DRIFT),
            out_sine: PolyOutput::default(),
            out_triangle: PolyOutput::default(),
            out_sawtooth: PolyOutput::default(),
            out_square: PolyOutput::default(),
            out_reset: PolyOutput::default(),
            drift: 0.0,
            drift_walks,
            drift_counter: 0,
            drift_voct_offsets: [0.0; 16],
        }
    })}

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        let v = p.get(params::frequency);
        self.freq_tracker.set_voct_offset(v);
        let inc = self.freq_converter.to_increment(self.freq_tracker.base_frequency());
        self.phase_acc.set_all_increments(inc);
        let t: OscFmType = p.get(params::fm_type);
        let fm_mode = match t {
            OscFmType::Linear => FMMode::Linear,
            OscFmType::Logarithmic => FMMode::Exponential,
        };
        self.freq_tracker.set_fm_mode(fm_mode);
        let v = p.get(params::drift);
        self.drift = v;
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_voct        = PolyInput::from_ports(inputs, 0);
        self.in_fm          = PolyInput::from_ports(inputs, 1);
        self.in_pulse_width = PolyInput::from_ports(inputs, 2);
        self.in_phase_mod   = PolyInput::from_ports(inputs, 3);
        self.in_sync        = PolyTriggerInput::from_ports(inputs, 4);
        self.out_sine     = PolyOutput::from_ports(outputs, 0);
        self.out_triangle = PolyOutput::from_ports(outputs, 1);
        self.out_sawtooth = PolyOutput::from_ports(outputs, 2);
        self.out_square   = PolyOutput::from_ports(outputs, 3);
        self.out_reset    = outputs[4].expect_poly_trigger();

        self.freq_tracker.voct_modulating = self.in_voct.is_connected();
        self.freq_tracker.fm_modulating   = self.in_fm.is_connected();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let voct = read_poly_or_zero(pool, &self.in_voct);
        let fm = read_poly_or_zero(pool, &self.in_fm);
        let phase_mod = read_poly_or_zero(pool, &self.in_phase_mod);

        let force_recalc = self.update_drift(pool);
        self.update_increments(&voct, &fm, force_recalc);

        let sync = if self.in_sync.is_connected() {
            self.in_sync.tick(pool)
        } else {
            [None; 16]
        };

        let outs = OutFlags {
            sine:  self.out_sine.is_connected(),
            tri:   self.out_triangle.is_connected(),
            saw:   self.out_sawtooth.is_connected(),
            sq:    self.out_square.is_connected(),
            reset: self.out_reset.is_connected(),
        };

        let any_sync = sync.iter().any(|s| s.is_some());
        if !outs.any() && !any_sync {
            self.phase_acc.advance_all();
            return;
        }

        let pw_connected = self.in_pulse_width.is_connected();
        let pulse_widths = read_poly_or_zero(pool, &self.in_pulse_width);
        let pm_connected = self.in_phase_mod.is_connected();

        let mut buf = VoiceBuffers::default();

        for i in 0..16 {
            let pm = if pm_connected { phase_mod[i].clamp(-1.0, 1.0) } else { 0.0 };
            let duty = if pw_connected {
                (0.5 + 0.5 * pulse_widths[i]).clamp(0.01, 0.99)
            } else {
                0.5
            };
            let dt = self.phase_acc.phase_increments[i];

            let ctx = VoiceCtx { dt, pm, duty };
            match sync[i] {
                Some(frac) => self.process_voice_synced(i, frac, &ctx, &outs, &mut buf),
                None => self.process_voice_free(i, &ctx, &outs, &mut buf),
            }
        }

        if outs.sine  { pool.write_poly(&self.out_sine,     buf.sine);  }
        if outs.tri   { pool.write_poly(&self.out_triangle, buf.tri);   }
        if outs.saw   { pool.write_poly(&self.out_sawtooth, buf.saw);   }
        if outs.sq    { pool.write_poly(&self.out_square,   buf.sq);    }
        if outs.reset { pool.write_poly(&self.out_reset,    buf.reset); }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[derive(Default)]
struct VoiceBuffers {
    sine:  [f32; 16],
    tri:   [f32; 16],
    saw:   [f32; 16],
    sq:    [f32; 16],
    reset: [f32; 16],
}

struct OutFlags { sine: bool, tri: bool, saw: bool, sq: bool, reset: bool }

struct VoiceCtx { dt: f32, pm: f32, duty: f32 }

impl OutFlags {
    fn any(&self) -> bool { self.sine || self.tri || self.saw || self.sq || self.reset }
}

fn read_poly_or_zero(pool: &mut CablePool<'_>, port: &PolyInput) -> [f32; 16] {
    if port.is_connected() { pool.read_poly(port) } else { [0.0; 16] }
}

fn wrap_unit(x: f32) -> f32 { x - x.floor() }

impl PolyOsc {
    /// Advance drift state. Returns true when offsets changed this sample.
    fn update_drift(&mut self, pool: &mut CablePool<'_>) -> bool {
        if self.drift <= 0.0 { return false; }
        self.drift_counter = self.drift_counter.wrapping_add(1);
        if self.drift_counter < DRIFT_PERIOD { return false; }
        self.drift_counter = 0;
        let global_val = pool.read_mono(&self.in_global_drift);
        let scale = HALF_SEMITONE_VOCT * 0.5 * self.drift;
        for i in 0..16 {
            let local_val = self.drift_walks[i].advance();
            self.drift_voct_offsets[i] = (global_val + local_val) * scale;
        }
        true
    }

    fn update_increments(&mut self, voct: &[f32; 16], fm: &[f32; 16], force_recalc: bool) {
        if self.freq_tracker.is_modulating() {
            for i in 0..16 {
                let freq = self.freq_tracker.compute_modulated(i, voct[i] + self.drift_voct_offsets[i], fm[i]);
                self.phase_acc.set_increment(i, self.freq_converter.to_increment(freq));
            }
        } else if force_recalc {
            let base_freq = self.freq_tracker.base_frequency();
            for i in 0..16 {
                let freq = base_freq * self.drift_voct_offsets[i].exp2();
                self.phase_acc.set_increment(i, self.freq_converter.to_increment(freq));
            }
        }
    }

    fn process_voice_synced(
        &mut self, i: usize, frac: f32, ctx: &VoiceCtx,
        outs: &OutFlags, buf: &mut VoiceBuffers,
    ) {
        let VoiceCtx { dt, pm, duty } = *ctx;
        let frac = frac.clamp(f32::MIN_POSITIVE, 1.0);
        let mut pre_raw = self.phase_acc.phases[i] + frac * dt;
        if pre_raw >= 1.0 { pre_raw -= 1.0; }
        let pre_read = wrap_unit(pre_raw + pm);

        self.phase_acc.sync_reset(i, frac);
        let post_raw = self.phase_acc.phases[i];
        let post_read = wrap_unit(post_raw + pm);

        if outs.sine { buf.sine[i] = lookup_sine(post_read); }
        if outs.tri  { buf.tri[i]  = 1.0 - 4.0 * (post_read - 0.5).abs(); }
        if outs.saw {
            let pre = 2.0 * pre_read - 1.0;
            let post = 2.0 * post_read - 1.0;
            buf.saw[i] = post + sync_blep_residual(post_raw, dt, pre - post);
        }
        if outs.sq {
            let pre = if pre_read < duty { 1.0 } else { -1.0 };
            let post = if post_read < duty { 1.0 } else { -1.0 };
            buf.sq[i] = post + sync_blep_residual(post_raw, dt, pre - post);
        }
    }

    fn process_voice_free(
        &mut self, i: usize, ctx: &VoiceCtx,
        outs: &OutFlags, buf: &mut VoiceBuffers,
    ) {
        let VoiceCtx { dt, pm, duty } = *ctx;
        let raw_phase = self.phase_acc.phases[i];
        let phase = wrap_unit(raw_phase + pm);

        if outs.sine { buf.sine[i] = lookup_sine(phase); }
        if outs.tri  { buf.tri[i]  = 1.0 - 4.0 * (phase - 0.5).abs(); }
        if outs.saw  { buf.saw[i]  = (2.0 * phase - 1.0) - polyblep(phase, dt); }
        if outs.sq {
            let raw = if phase < duty { 1.0 } else { -1.0 };
            let blep = polyblep(phase, dt) - polyblep((phase - duty).rem_euclid(1.0), dt);
            buf.sq[i] = raw + blep;
        }

        let next = self.phase_acc.phases[i] + dt;
        if next >= 1.0 {
            self.phase_acc.phases[i] = next - 1.0;
            buf.reset[i] = if dt > 0.0 {
                (1.0 - self.phase_acc.phases[i] / dt).clamp(f32::MIN_POSITIVE, 1.0)
            } else { 1.0 };
        } else {
            self.phase_acc.phases[i] = next;
        }
    }
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
        h.init_pool(CableValue::poly([99.0; 16]));
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

    // ── reset_out / sync (0636, ADR 0047) ────────────────────────────────

    #[test]
    fn reset_out_emits_per_voice_wrap_frac() {
        let sr = 48_000.0_f32;
        let freq = 200.0_f32;
        let voct = (freq / C0_FREQ).log2();
        let mut h = ModuleHarness::build_with_env::<PolyOsc>(
            params!["frequency" => voct],
            env(sr, 2),
        );
        h.disconnect_input("fm");
        h.disconnect_input("pulse_width_cv");
        h.disconnect_input("phase_mod");
        h.disconnect_input("sync");
        let mut voct_arr = [voct; 16];
        voct_arr[1] = voct + 1.0; // voice 1 one octave up
        h.set_poly("voct", voct_arr);
        let n = 400_usize;
        let frames = h.run_poly(n, "reset_out");
        let (mut w0, mut w1) = (0usize, 0usize);
        for f in &frames {
            if f[0] > 0.0 { w0 += 1; }
            if f[1] > 0.0 { w1 += 1; }
        }
        assert!(w1 > w0, "voice 1 should wrap more often than voice 0; got v0={w0} v1={w1}");
    }

    #[test]
    fn poly_sync_is_per_voice() {
        let sr = 48_000.0_f32;
        let freq = 200.0_f32;
        let voct = (freq / C0_FREQ).log2();
        let mut h = ModuleHarness::build_with_env::<PolyOsc>(
            params!["frequency" => voct],
            env(sr, 2),
        );
        h.disconnect_input("fm");
        h.disconnect_input("pulse_width_cv");
        h.disconnect_input("phase_mod");
        h.set_poly("voct", [voct; 16]);

        let mut sync_arr = [0.0f32; 16];
        sync_arr[0] = 0.5;
        h.set_poly("sync", sync_arr);
        let _ = h.run_poly(8, "sawtooth");
        h.set_poly("sync", sync_arr);
        h.tick();
        let after = h.read_poly("sawtooth");

        let mut h2 = ModuleHarness::build_with_env::<PolyOsc>(
            params!["frequency" => voct],
            env(sr, 2),
        );
        h2.disconnect_input("fm");
        h2.disconnect_input("pulse_width_cv");
        h2.disconnect_input("phase_mod");
        h2.set_poly("voct", [voct; 16]);
        h2.set_poly("sync", [0.0; 16]);
        let _ = h2.run_poly(8, "sawtooth");
        h2.set_poly("sync", [0.0; 16]);
        h2.tick();
        let no_sync = h2.read_poly("sawtooth");

        assert!(after[0] < 0.0, "voice 0 should be in negative saw post-sync; got {}", after[0]);
        assert!(
            (after[1] - no_sync[1]).abs() < 1e-5,
            "voice 1 affected by sync[0]"
        );
    }
}
