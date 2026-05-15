use patches_core::{
    AudioEnvironment, CablePool, CountAxis, InputPort, InstanceId, Module,
    ModuleDescriptor, ModuleDescriptorTemplate, OutputPort, ParameterKind, ParameterTemplate,
    PolyGateInput, PolyInput, PolyOutput, PortTemplate,
};
use patches_core::{StructuralParams, BuildError};
use patches_core::cables::PolyTriggerInput;
use patches_core::module_params;
use patches_core::param_frame::ParamView;
use patches_dsp::{AdsrCore, AdsrShape};

use crate::adsr::AdsrShapeParam;
use crate::common::frequency::{C0_FREQ, FMMode, PolyFrequencyConverter, PolyFrequencyChangeTracker};
use crate::common::phase_accumulator::PolyPhaseAccumulator;
use crate::op::{op_waveform, OpPhaseReset, OpWaveform};
use crate::oscillator::OscFmType;

module_params! {
    PolyOp {
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

/// Polyphonic FM operator: 16-voice [`Op`](crate::op::Op).
///
/// One phase accumulator and one [`AdsrCore`] per voice; channel `i` of every
/// poly input drives voice `i`. See `Op` for the per-voice semantics — phase
/// modulation, the 2-sample rolling-average feedback smoother, trigger-driven
/// phase reset, and the 8 analytic waveforms are all per-voice.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `voct` | poly | V/oct pitch CV per voice |
/// | `fm` | poly | Frequency modulation per voice |
/// | `phase_mod` | poly | Phase modulation offset per voice |
/// | `feedback` | poly | Feedback input per voice; 2-sample rolling avg added to phase |
/// | `trigger` | poly_trigger | Per-voice rising edge starts the envelope; optionally resets phase |
/// | `gate` | poly | Per-voice gate |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | poly | Waveform × envelope per voice |
/// | `env` | poly | Raw envelope level per voice |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `frequency` | float | -4.0 -- 12.0 | `0.0` | Base pitch as V/oct offset from C0 |
/// | `fm_type` | enum | linear, logarithmic | `linear` | FM mode |
/// | `waveform` | enum | sine, half_sine, abs_sine, quarter_sine, alt_sine, alt_abs_sine, square, saw | `sine` | Output waveform shape |
/// | `delay` | float | 0.0 -- 10.0 | `0.0` | Pre-attack hold time |
/// | `attack` | float | 0.001 -- 10.0 | `0.01` | Attack time |
/// | `decay` | float | 0.001 -- 10.0 | `0.1` | Decay time |
/// | `sustain` | float | 0.0 -- 1.0 | `0.7` | Sustain level |
/// | `release` | float | 0.001 -- 10.0 | `0.3` | Release time |
/// | `shape` | enum | linear, exponential | `linear` | Envelope shape |
/// | `phase_reset` | enum | off, on | `off` | Snap phase on trigger |
/// | `start_phase` | float | 0.0 -- 1.0 | `0.0` | Phase value applied on reset |
pub struct PolyOp {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    phase_acc: PolyPhaseAccumulator,
    freq_converter: PolyFrequencyConverter,
    freq_tracker: PolyFrequencyChangeTracker,
    adsrs: [AdsrCore; 16],
    waveform: OpWaveform,
    phase_reset: bool,
    start_phase: f32,
    fb_z1: [f32; 16],
    in_voct: PolyInput,
    in_fm: PolyInput,
    in_phase_mod: PolyInput,
    in_feedback: PolyInput,
    in_trigger: PolyTriggerInput,
    in_gate: PolyGateInput,
    out_signal: PolyOutput,
    out_env: PolyOutput,
}

impl Module for PolyOp {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "PolyOp",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[
                PortTemplate::poly("voct"),
                PortTemplate::poly("fm"),
                PortTemplate::poly("phase_mod"),
                PortTemplate::poly("feedback"),
                PortTemplate::poly_trigger("trigger"),
                PortTemplate::poly("gate"),
            ],
            per_axis_inputs: &[],
            global_outputs: &[
                PortTemplate::poly("out"),
                PortTemplate::poly("env"),
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
        let sr = audio_environment.sample_rate;
        Ok(Self {
            instance_id,
            descriptor,
            phase_acc: PolyPhaseAccumulator::new(),
            freq_converter: PolyFrequencyConverter::new(sr),
            freq_tracker: PolyFrequencyChangeTracker::new(C0_FREQ),
            adsrs: std::array::from_fn(|_| AdsrCore::new(sr)),
            waveform: OpWaveform::Sine,
            phase_reset: false,
            start_phase: 0.0,
            fb_z1: [0.0; 16],
            in_voct: PolyInput::default(),
            in_fm: PolyInput::default(),
            in_phase_mod: PolyInput::default(),
            in_feedback: PolyInput::default(),
            in_trigger: PolyTriggerInput::default(),
            in_gate: PolyGateInput::default(),
            out_signal: PolyOutput::default(),
            out_env: PolyOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.freq_tracker.set_voct_offset(p.get(params::frequency));
        let inc = self.freq_converter.to_increment(self.freq_tracker.base_frequency());
        self.phase_acc.set_all_increments(inc);
        let t: OscFmType = p.get(params::fm_type);
        self.freq_tracker.set_fm_mode(match t {
            OscFmType::Linear => FMMode::Linear,
            OscFmType::Logarithmic => FMMode::Exponential,
        });
        self.waveform = p.get(params::waveform);
        let attack = p.get(params::attack);
        let decay = p.get(params::decay);
        let sustain = p.get(params::sustain);
        let release = p.get(params::release);
        let delay = p.get(params::delay);
        let shape_p: AdsrShapeParam = p.get(params::shape);
        let shape: AdsrShape = shape_p.into();
        for voice in &mut self.adsrs {
            voice.set_params(attack, decay, sustain, release);
            voice.set_delay(delay);
            voice.set_shape(shape);
        }
        let pr: OpPhaseReset = p.get(params::phase_reset);
        self.phase_reset = matches!(pr, OpPhaseReset::On);
        self.start_phase = p.get(params::start_phase).clamp(0.0, 1.0);
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_voct      = PolyInput::from_ports(inputs, 0);
        self.in_fm        = PolyInput::from_ports(inputs, 1);
        self.in_phase_mod = PolyInput::from_ports(inputs, 2);
        self.in_feedback  = PolyInput::from_ports(inputs, 3);
        self.in_trigger   = PolyTriggerInput::from_ports(inputs, 4);
        self.in_gate      = PolyGateInput::from_ports(inputs, 5);
        self.out_signal = PolyOutput::from_ports(outputs, 0);
        self.out_env    = PolyOutput::from_ports(outputs, 1);
        self.freq_tracker.voct_modulating = self.in_voct.is_connected();
        self.freq_tracker.fm_modulating   = self.in_fm.is_connected();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let triggers = self.in_trigger.tick(pool);
        let gates    = self.in_gate.tick(pool);

        let voct = if self.in_voct.is_connected() { pool.read_poly(&self.in_voct) } else { [0.0; 16] };
        let fm = if self.in_fm.is_connected() { pool.read_poly(&self.in_fm) } else { [0.0; 16] };
        let pm = if self.in_phase_mod.is_connected() { pool.read_poly(&self.in_phase_mod) } else { [0.0; 16] };
        let fb = if self.in_feedback.is_connected() { pool.read_poly(&self.in_feedback) } else { [0.0; 16] };

        if self.freq_tracker.is_modulating() {
            for i in 0..16 {
                let freq = self.freq_tracker.compute_modulated(i, voct[i], fm[i]);
                self.phase_acc.set_increment(i, self.freq_converter.to_increment(freq));
            }
        }

        let mut signal_out = [0.0f32; 16];
        let mut env_out = [0.0f32; 16];
        for i in 0..16 {
            let triggered = triggers[i].is_some();
            if triggered && self.phase_reset {
                self.phase_acc.phases[i] = self.start_phase;
            }
            let fb_avg = (fb[i] + self.fb_z1[i]) * 0.5;
            self.fb_z1[i] = fb[i];
            let read_phase = (self.phase_acc.phases[i] + pm[i] + fb_avg).rem_euclid(1.0);
            let raw = op_waveform(self.waveform, read_phase);
            let env = self.adsrs[i].tick(triggered, gates[i].is_high);
            env_out[i] = env;
            signal_out[i] = raw * env;
        }

        if self.out_signal.is_connected() { pool.write_poly(&self.out_signal, signal_out); }
        if self.out_env.is_connected()    { pool.write_poly(&self.out_env, env_out); }

        self.phase_acc.advance_all();
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::AudioEnvironment;
    use patches_core::test_support::{assert_within, ModuleHarness, params};

    fn env(sample_rate: f32, voices: usize) -> AudioEnvironment {
        AudioEnvironment { sample_rate, poly_voices: voices, periodic_update_interval: 32, hosted: false }
    }

    fn arr(val: f32, voice: usize) -> [f32; 16] {
        let mut a = [0.0f32; 16];
        a[voice] = val;
        a
    }

    /// Each voice runs an independent ADSR and phase accumulator.
    /// Voice 0 gated, voice 1 silent → voice 1 output stays at 0.
    #[test]
    fn voice_envelopes_are_independent() {
        let mut h = ModuleHarness::build_with_env::<PolyOp>(
            params!["attack" => 0.001_f32, "sustain" => 1.0_f32],
            env(C0_FREQ * 100.0, 2),
        );
        h.disconnect_all_inputs();
        h.set_poly("trigger", arr(1.0, 0));
        h.set_poly("gate", arr(1.0, 0));
        h.tick();
        h.set_poly("trigger", [0.0; 16]);
        for _ in 0..30 { h.tick(); }
        let out = h.read_poly("out");
        assert!(out[0].abs() > 0.0, "voice 0 should be sounding; got {}", out[0]);
        assert_eq!(out[1], 0.0, "voice 1 silent without trigger; got {}", out[1]);
    }

    /// Per-voice voct: voice 1 at +1 oct runs twice as fast and reaches phase 0.5
    /// (sine zero-cross) when voice 0 reaches phase 0.25 (sine peak).
    #[test]
    fn voct_drives_voices_independently() {
        let period = 100_usize;
        let mut h = ModuleHarness::build_with_env::<PolyOp>(
            params!["attack" => 0.001_f32, "sustain" => 1.0_f32],
            env(C0_FREQ * period as f32, 2),
        );
        h.disconnect_input("fm");
        h.disconnect_input("phase_mod");
        h.disconnect_input("feedback");
        let mut voct = [0.0f32; 16];
        voct[1] = 1.0;
        h.set_poly("voct", voct);
        let mut trig = [0.0f32; 16];
        trig[0] = 1.0; trig[1] = 1.0;
        let mut gate = [0.0f32; 16];
        gate[0] = 1.0; gate[1] = 1.0;
        h.set_poly("trigger", trig);
        h.set_poly("gate", gate);
        h.tick();
        h.set_poly("trigger", [0.0; 16]);
        for _ in 0..(period / 4 - 1) { h.tick(); }
        let out = h.read_poly("out");
        assert!(out[0] > 0.9, "voice 0 at phase ~0.25 should be near +1; got {}", out[0]);
        assert_within!(0.0_f32, out[1], 0.2_f32);
    }

    /// `phase_reset = on` snaps the targeted voice's phase only. Settle env on
    /// voice 0 at sustain, then re-trigger voice 0 to invoke phase reset; voice
    /// 1 is never triggered and stays silent.
    #[test]
    fn phase_reset_is_per_voice() {
        let mut h = ModuleHarness::build_with_env::<PolyOp>(
            params![
                "attack" => 0.001_f32, "sustain" => 1.0_f32,
                "phase_reset" => OpPhaseReset::On, "start_phase" => 0.25_f32,
            ],
            env(44100.0, 2),
        );
        h.disconnect_all_inputs();
        let mut gate = [0.0f32; 16];
        gate[0] = 1.0; gate[1] = 1.0;
        h.set_poly("gate", gate);
        h.set_poly("trigger", arr(1.0, 0));
        h.tick();
        h.set_poly("trigger", [0.0; 16]);
        for _ in 0..100 { h.tick(); }
        h.set_poly("trigger", arr(1.0, 0));
        h.tick();
        let out = h.read_poly("out");
        assert!(out[0] > 0.95, "voice 0 should snap to phase 0.25 (sin near +1); got {}", out[0]);
        assert_eq!(out[1], 0.0, "voice 1 never triggered, must be silent");
    }

    #[test]
    fn disconnected_outputs_are_not_written() {
        let mut h = ModuleHarness::build_with_env::<PolyOp>(params![], env(44100.0, 1));
        h.disconnect_all_inputs();
        h.disconnect_all_outputs();
        h.init_pool(patches_core::CableValue::poly([99.0; 16]));
        h.tick();
        for name in &["out", "env"] {
            let out = h.read_poly(name);
            assert_eq!(99.0_f32, out[0], "output '{name}' voice 0 written despite disconnection");
        }
    }
}
