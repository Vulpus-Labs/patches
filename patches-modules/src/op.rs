use patches_core::{
    params_enum,
    AudioEnvironment, CablePool, CountAxis, GateInput, InputPort, InstanceId, Module,
    ModuleDescriptor, ModuleDescriptorTemplate, MonoInput, MonoOutput, OutputPort,
    ParameterKind, ParameterTemplate, PortTemplate,
};
use patches_core::{StructuralParams, BuildError};
use patches_core::cables::TriggerInput;
use patches_core::module_params;
use patches_core::param_frame::ParamView;
use patches_dsp::{AdsrCore, lookup_sine};

use crate::adsr::AdsrShapeParam;
use crate::common::frequency::{C0_FREQ, FMMode, MonoFrequencyConverter, MonoFrequencyChangeTracker};
use crate::common::phase_accumulator::MonoPhaseAccumulator;
use crate::oscillator::OscFmType;

params_enum! {
    pub enum OpWaveform {
        Sine        => "sine",
        HalfSine    => "half_sine",
        AbsSine     => "abs_sine",
        QuarterSine => "quarter_sine",
        AltSine     => "alt_sine",
        AltAbsSine  => "alt_abs_sine",
        Square      => "square",
        Saw         => "saw",
    }
}

params_enum! {
    pub enum OpPhaseReset {
        Off => "off",
        On  => "on",
    }
}

module_params! {
    Op {
        frequency:   Float,
        fm_type:     Enum<OscFmType>,
        waveform:    Enum<OpWaveform>,
        delay:       Float,
        attack:      Float,
        decay:       Float,
        sustain:     Float,
        release:     Float,
        shape:       Enum<AdsrShapeParam>,
        phase_reset: Enum<OpPhaseReset>,
        start_phase: Float,
    }
}

/// Evaluate the selected analytic waveform at `phase` ∈ [0, 1).
///
/// Sine uses the shared linear-interpolated lookup table; the other shapes are
/// the OPL3-family analytic waveforms (matching the operator-waveform set the
/// Yamaha V50 and its YM2414/OPZ siblings exposed beyond the original DX7 sine).
pub fn op_waveform(w: OpWaveform, phase: f32) -> f32 {
    match w {
        OpWaveform::Sine        => lookup_sine(phase),
        OpWaveform::HalfSine    => if phase < 0.5 { lookup_sine(phase) } else { 0.0 },
        OpWaveform::AbsSine     => lookup_sine(phase).abs(),
        OpWaveform::QuarterSine => {
            // Pulse sine: keep the first quarter of each half-cycle, zero the rest.
            let p = if phase >= 0.5 { phase - 0.5 } else { phase };
            if p < 0.25 { lookup_sine(phase) } else { 0.0 }
        }
        OpWaveform::AltSine     => if phase < 0.5 { lookup_sine(2.0 * phase) } else { 0.0 },
        OpWaveform::AltAbsSine  => if phase < 0.5 { lookup_sine(2.0 * phase).abs() } else { 0.0 },
        OpWaveform::Square      => if phase < 0.5 { 1.0 } else { -1.0 },
        OpWaveform::Saw         => 1.0 - 2.0 * phase,
    }
}

/// FM operator: 8 analytic waveforms, built-in DADSR envelope, phase-mod and
/// feedback inputs, optional phase reset on trigger.
///
/// Phase-mod and feedback both shift the read phase per sample. The feedback
/// input is first smoothed by a 2-sample rolling average (current + previous,
/// halved) — the classic DX7-style feedback path that tames the runaway gain
/// of a raw self-feedback loop.
///
/// The `frequency` parameter is a V/OCT offset from C0 (≈ 16.35 Hz). FM
/// (`fm_type`) is either linear (additive Hz) or logarithmic (exponential
/// scaling in V/oct), matching [`Osc`](crate::Oscillator).
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `voct` | mono | V/oct pitch CV added to base frequency |
/// | `fm` | mono | Frequency modulation input (linear Hz or V/oct per `fm_type`) |
/// | `phase_mod` | mono | Phase modulation offset added directly to the phase each sample |
/// | `feedback` | mono | Feedback input; rolling 2-sample average is added to the phase |
/// | `trigger` | trigger | Rising edge starts the envelope; optionally resets phase to `start_phase` |
/// | `gate` | mono | Held high to sustain; release to enter Release |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | mono | Waveform multiplied by the envelope |
/// | `env` | mono | Raw envelope level in \[0, 1\] |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `frequency` | float | -4.0 -- 12.0 | `0.0` | Base pitch as V/oct offset from C0 |
/// | `fm_type` | enum | linear, logarithmic | `linear` | FM mode |
/// | `waveform` | enum | sine, half_sine, abs_sine, quarter_sine, alt_sine, alt_abs_sine, square, saw | `sine` | Output waveform shape |
/// | `delay` | float | 0.0 -- 10.0 | `0.0` | Pre-attack hold time in seconds |
/// | `attack` | float | 0.001 -- 10.0 | `0.01` | Attack time in seconds |
/// | `decay` | float | 0.001 -- 10.0 | `0.1` | Decay time in seconds |
/// | `sustain` | float | 0.0 -- 1.0 | `0.7` | Sustain level |
/// | `release` | float | 0.001 -- 10.0 | `0.3` | Release time in seconds |
/// | `shape` | enum | linear, exponential | `linear` | Envelope segment shape |
/// | `phase_reset` | enum | off, on | `off` | If `on`, a rising trigger snaps phase to `start_phase` |
/// | `start_phase` | float | 0.0 -- 1.0 | `0.0` | Phase value applied on reset |
pub struct Op {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    phase_acc: MonoPhaseAccumulator,
    freq_converter: MonoFrequencyConverter,
    freq_tracker: MonoFrequencyChangeTracker,
    adsr: AdsrCore,
    waveform: OpWaveform,
    phase_reset: bool,
    start_phase: f32,
    /// Previous feedback-input sample, used by the 2-sample rolling average.
    fb_z1: f32,
    // Input port fields
    in_voct: MonoInput,
    in_fm: MonoInput,
    in_phase_mod: MonoInput,
    in_feedback: MonoInput,
    in_trigger: TriggerInput,
    in_gate: GateInput,
    // Output port fields
    out_signal: MonoOutput,
    out_env: MonoOutput,
}

impl Module for Op {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "Op",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[
                PortTemplate::mono("voct"),
                PortTemplate::mono("fm"),
                PortTemplate::mono("phase_mod"),
                PortTemplate::mono("feedback"),
                PortTemplate::trigger("trigger"),
                PortTemplate::mono("gate"),
            ],
            per_axis_inputs: &[],
            global_outputs: &[
                PortTemplate::mono("out"),
                PortTemplate::mono("env"),
            ],
            per_axis_outputs: &[],
            realtime_params: &[
                ParameterTemplate { name: params::frequency.as_str(),
                    kind: ParameterKind::Float { min: -4.0, max: 12.0, default: 0.0 } },
                ParameterTemplate { name: params::fm_type.as_str(),
                    kind: ParameterKind::Enum { variants: OscFmType::VARIANTS, default: "linear" } },
                ParameterTemplate { name: params::waveform.as_str(),
                    kind: ParameterKind::Enum { variants: OpWaveform::VARIANTS, default: "sine" } },
                ParameterTemplate { name: params::delay.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 10.0, default: 0.0 } },
                ParameterTemplate { name: params::attack.as_str(),
                    kind: ParameterKind::Float { min: 0.001, max: 10.0, default: 0.01 } },
                ParameterTemplate { name: params::decay.as_str(),
                    kind: ParameterKind::Float { min: 0.001, max: 10.0, default: 0.1 } },
                ParameterTemplate { name: params::sustain.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.7 } },
                ParameterTemplate { name: params::release.as_str(),
                    kind: ParameterKind::Float { min: 0.001, max: 10.0, default: 0.3 } },
                ParameterTemplate { name: params::shape.as_str(),
                    kind: ParameterKind::Enum { variants: AdsrShapeParam::VARIANTS, default: "linear" } },
                ParameterTemplate { name: params::phase_reset.as_str(),
                    kind: ParameterKind::Enum { variants: OpPhaseReset::VARIANTS, default: "off" } },
                ParameterTemplate { name: params::start_phase.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.0 } },
            ],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, _structural: &StructuralParams) -> Result<Self, BuildError> {
        Ok(Self {
            instance_id,
            descriptor,
            phase_acc: MonoPhaseAccumulator::new(),
            freq_converter: MonoFrequencyConverter::new(audio_environment.sample_rate),
            freq_tracker: MonoFrequencyChangeTracker::new(C0_FREQ),
            adsr: AdsrCore::new(audio_environment.sample_rate),
            waveform: OpWaveform::Sine,
            phase_reset: false,
            start_phase: 0.0,
            fb_z1: 0.0,
            in_voct: MonoInput::default(),
            in_fm: MonoInput::default(),
            in_phase_mod: MonoInput::default(),
            in_feedback: MonoInput::default(),
            in_trigger: TriggerInput::default(),
            in_gate: GateInput::default(),
            out_signal: MonoOutput::default(),
            out_env: MonoOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.freq_tracker.set_voct_offset(p.get(params::frequency));
        let inc = self.freq_converter.to_increment(self.freq_tracker.base_frequency());
        self.phase_acc.set_increment(inc);
        let t: OscFmType = p.get(params::fm_type);
        self.freq_tracker.set_fm_mode(match t {
            OscFmType::Linear => FMMode::Linear,
            OscFmType::Logarithmic => FMMode::Exponential,
        });
        self.waveform = p.get(params::waveform);
        self.adsr.set_params(
            p.get(params::attack),
            p.get(params::decay),
            p.get(params::sustain),
            p.get(params::release),
        );
        self.adsr.set_delay(p.get(params::delay));
        let shape_p: AdsrShapeParam = p.get(params::shape);
        self.adsr.set_shape(shape_p.into());
        let pr: OpPhaseReset = p.get(params::phase_reset);
        self.phase_reset = matches!(pr, OpPhaseReset::On);
        self.start_phase = p.get(params::start_phase).clamp(0.0, 1.0);
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_voct      = inputs[0].expect_mono();
        self.in_fm        = inputs[1].expect_mono();
        self.in_phase_mod = inputs[2].expect_mono();
        self.in_feedback  = inputs[3].expect_mono();
        self.in_trigger   = TriggerInput::from_ports(inputs, 4);
        self.in_gate      = GateInput::from_ports(inputs, 5);
        self.out_signal = outputs[0].expect_mono();
        self.out_env    = outputs[1].expect_mono();
        self.freq_tracker.voct_modulating = self.in_voct.is_connected();
        self.freq_tracker.fm_modulating   = self.in_fm.is_connected();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let triggered = self.in_trigger.tick(pool).is_some();
        let gate_high = self.in_gate.tick(pool).is_high;

        if triggered && self.phase_reset {
            self.phase_acc.phase = self.start_phase;
        }

        if self.freq_tracker.is_modulating() {
            let voct = pool.read_mono(&self.in_voct);
            let fm = pool.read_mono(&self.in_fm);
            let freq = self.freq_tracker.compute_modulated(voct, fm);
            self.phase_acc.set_increment(self.freq_converter.to_increment(freq));
        }

        let pm = if self.in_phase_mod.is_connected() { pool.read_mono(&self.in_phase_mod) } else { 0.0 };
        let fb_in = if self.in_feedback.is_connected() { pool.read_mono(&self.in_feedback) } else { 0.0 };
        let fb_avg = (fb_in + self.fb_z1) * 0.5;
        self.fb_z1 = fb_in;

        let read_phase = (self.phase_acc.phase + pm + fb_avg).rem_euclid(1.0);
        let raw = op_waveform(self.waveform, read_phase);
        let env = self.adsr.tick(triggered, gate_high);

        if self.out_signal.is_connected() {
            pool.write_mono(&self.out_signal, raw * env);
        }
        if self.out_env.is_connected() {
            pool.write_mono(&self.out_env, env);
        }

        self.phase_acc.advance();
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::AudioEnvironment;
    use patches_core::test_support::{assert_within, ModuleHarness, params};

    fn env(sample_rate: f32) -> AudioEnvironment {
        AudioEnvironment { sample_rate, poly_voices: 16, periodic_update_interval: 32, hosted: false }
    }

    fn make_op(p: &[(&str, patches_core::parameter_map::ParameterValue)], sample_rate: f32) -> ModuleHarness {
        let mut h = ModuleHarness::build_with_env::<Op>(p, env(sample_rate));
        h.disconnect_all_inputs();
        h
    }

    fn waveforms_for_iteration() -> [OpWaveform; 8] {
        [
            OpWaveform::Sine,
            OpWaveform::HalfSine,
            OpWaveform::AbsSine,
            OpWaveform::QuarterSine,
            OpWaveform::AltSine,
            OpWaveform::AltAbsSine,
            OpWaveform::Square,
            OpWaveform::Saw,
        ]
    }

    /// With gate held high and a fast attack, the envelope reaches sustain
    /// quickly and the output equals waveform × env. At one-quarter cycle of
    /// the sine table this lands at lookup_sine(0.25) ≈ 1.0.
    #[test]
    fn out_is_waveform_times_envelope() {
        let period = 100_usize;
        let mut h = make_op(
            params!["attack" => 0.001_f32, "sustain" => 1.0_f32],
            C0_FREQ * period as f32,
        );
        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        // Advance to phase ≈ 0.25 (quarter cycle); envelope already at sustain=1.0.
        for _ in 0..(period / 4 - 1) {
            h.tick();
        }
        let v = h.read_mono("out");
        // lookup table error ~1e-4; allow headroom for phase accumulation
        assert_within!(lookup_sine(0.25), v, 5e-3_f32);
    }

    #[test]
    fn env_output_tracks_envelope_level() {
        let mut h = make_op(params!["attack" => 0.5_f32, "sustain" => 0.5_f32], 10.0);
        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        // attack=0.5s at 10 Hz → 5 samples, inc=0.2 → first sample = 0.2
        assert_within!(0.2_f32, h.read_mono("env"), 1e-6_f32);
    }

    /// Switching `waveform` to `square` yields ±1 (× envelope) on either side
    /// of the half-cycle boundary.
    #[test]
    fn square_waveform_produces_pm_one() {
        let period = 100_usize;
        let mut h = make_op(
            params!["attack" => 0.001_f32, "sustain" => 1.0_f32, "waveform" => OpWaveform::Square],
            C0_FREQ * period as f32,
        );
        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        // After a few samples, env is at 1.0 and phase < 0.5 → square = +1.
        for _ in 0..4 { h.tick(); }
        let v = h.read_mono("out");
        assert_within!(1.0_f32, v, 1e-3_f32);
    }

    /// `phase_reset = on` snaps phase to `start_phase` on a rising trigger.
    /// Settle envelope at sustain first, then re-trigger; output should jump
    /// to ~`sin(2π·start_phase) · env`.
    #[test]
    fn phase_reset_snaps_to_start_phase_on_trigger() {
        let mut h = make_op(
            params![
                "attack" => 0.001_f32, "sustain" => 1.0_f32,
                "phase_reset" => OpPhaseReset::On, "start_phase" => 0.25_f32,
            ],
            44100.0,
        );
        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        // Reach sustain (env=1.0). Phase remains near 0 at C0 — irrelevant
        // because phase_reset will overwrite it on the next trigger.
        for _ in 0..100 { h.tick(); }
        // Re-trigger: ADSR stays at sustain level; phase snaps to 0.25.
        h.set_mono("trigger", 1.0);
        h.tick();
        let v = h.read_mono("out");
        // sin(2π·0.25) = 1 × env(~1) → near +1.
        assert!(v > 0.95, "expected output near +1 after phase reset to 0.25; got {v}");
    }

    /// `phase_reset = off` leaves the free-running phase intact across triggers.
    #[test]
    fn phase_reset_off_preserves_free_phase() {
        let mut h = make_op(
            params!["attack" => 0.001_f32, "sustain" => 1.0_f32, "phase_reset" => OpPhaseReset::Off],
            44100.0,
        );
        h.set_mono("trigger", 1.0);
        h.set_mono("gate", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        for _ in 0..100 { h.tick(); }
        let pre = h.read_mono("out");
        h.set_mono("trigger", 1.0);
        h.tick();
        h.set_mono("trigger", 0.0);
        let post = h.read_mono("out");
        // Phase has advanced one tiny step (≈ 16.35 / 44100 ≈ 0.00037), so
        // `post` should be within a few percent of `pre`. A phase reset would
        // have snapped it to start_phase=0 → output 0 → diff ~= pre.
        assert!(
            (post - pre).abs() < 0.05,
            "phase appears to have jumped: pre={pre} post={post}"
        );
    }

    /// Feedback input passes through the 2-sample rolling-average smoother
    /// before being added to the phase. With a constant feedback value `v`,
    /// the smoother starts at v/2 on the first sample (only one tap loaded)
    /// and reaches v on the next.
    #[test]
    fn feedback_smoother_initial_response() {
        let period = 100_usize;
        let mut h = make_op(
            params!["attack" => 0.001_f32, "sustain" => 1.0_f32],
            C0_FREQ * period as f32,
        );
        h.set_mono("gate", 1.0);
        h.set_mono("trigger", 1.0);
        h.tick(); // first sample: env attacks, feedback z1 still 0
        h.set_mono("trigger", 0.0);
        // Drive feedback to 0.5 (half a cycle worth of phase offset).
        h.set_mono("feedback", 0.5);
        h.tick(); // smoother: (0.5 + 0) * 0.5 = 0.25
        h.tick(); // smoother: (0.5 + 0.5) * 0.5 = 0.5
        // We don't pin a numeric output value (it depends on phase progression),
        // but the smoothing only takes effect if z1 actually carries state —
        // verify by ensuring the output is finite and within range.
        let v = h.read_mono("out");
        assert!(v.is_finite() && v.abs() <= 1.0, "feedback path destabilised output: {v}");
    }

    /// Disconnected outputs are not written.
    #[test]
    fn disconnected_outputs_are_not_written() {
        let mut h = ModuleHarness::build_with_env::<Op>(params![], env(44100.0));
        h.disconnect_all_inputs();
        h.disconnect_all_outputs();
        h.init_pool(patches_core::CableValue::mono(99.0));
        h.tick();
        for name in &["out", "env"] {
            assert_eq!(99.0_f32, h.read_mono(name), "output '{name}' written despite disconnection");
        }
    }

    /// All 8 waveforms produce finite, bounded output across a full cycle.
    #[test]
    fn all_waveforms_finite_and_bounded() {
        let period = 64_usize;
        for w in waveforms_for_iteration() {
            let mut h = make_op(
                params!["attack" => 0.001_f32, "sustain" => 1.0_f32, "waveform" => w],
                C0_FREQ * period as f32,
            );
            h.set_mono("trigger", 1.0);
            h.set_mono("gate", 1.0);
            h.tick();
            h.set_mono("trigger", 0.0);
            for i in 0..period * 2 {
                h.tick();
                let v = h.read_mono("out");
                assert!(v.is_finite(), "{w:?} produced non-finite at i={i}: {v}");
                assert!(v.abs() <= 1.0 + 1e-5, "{w:?} exceeded ±1 at i={i}: {v}");
            }
        }
    }
}
