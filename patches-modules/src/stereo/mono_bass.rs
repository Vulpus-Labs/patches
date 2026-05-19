//! Low-frequency mono summer with Linkwitz-Riley 4th-order crossover
//! (ADR 0076).
//!
//! Splits the stereo input into a low-frequency band and a high-frequency
//! band at `cutoff` using LR4 filters. The lows are summed to mono and
//! sent identically to both output channels; the highs pass through
//! per-channel. The mono-bass component is computed once by LP-filtering
//! `(L + R) / 2` (linear-phase combination + filter linearity gives the
//! same result as filtering each channel and then averaging, with one
//! fewer biquad pair). The high band is HP-filtered per channel so
//! stereo image above the crossover is preserved.
//!
//! Useful for vinyl-safe masters, club-system protection, and any
//! source whose low end you do not trust to be summable.
//!
//! # Inputs
//!
//! | Port | Kind   | Description   |
//! |------|--------|---------------|
//! | `in` | stereo | Source signal |
//!
//! # Outputs
//!
//! | Port  | Kind   | Description                              |
//! |-------|--------|------------------------------------------|
//! | `out` | stereo | High-band stereo + low-band mono on both |
//!
//! # Parameters
//!
//! | Name     | Type  | Range    | Default | Description                |
//! |----------|-------|----------|---------|----------------------------|
//! | `cutoff` | float | 20..2000 | `120`   | Crossover frequency in Hz  |

use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate};
use patches_core::param_frame::ParamView;
use patches_core::{
    module_params, AudioEnvironment, BuildError, CablePool, InputPort, InstanceId, Module,
    ModuleDescriptor, OutputPort, ParameterKind, StereoInput, StereoOutput, StructuralParams,
};

use crate::filter::{compute_biquad_highpass, compute_biquad_lowpass};
use crate::stereo::common::lr4::Lr4Filter;

module_params! {
    MonoBass {
        cutoff: Float,
    }
}

const DEFAULT_CUTOFF_HZ: f32 = 120.0;
const MIN_CUTOFF_HZ: f32 = 20.0;
const MAX_CUTOFF_HZ: f32 = 2000.0;

pub struct MonoBass {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    cutoff_hz: f32,

    in_stereo: StereoInput,
    out_stereo: StereoOutput,

    lp_mono: Lr4Filter,
    hp_left: Lr4Filter,
    hp_right: Lr4Filter,
}

impl MonoBass {
    fn recompute_coeffs(&mut self) {
        let lp = compute_biquad_lowpass(self.cutoff_hz, 0.0, self.sample_rate);
        let hp = compute_biquad_highpass(self.cutoff_hz, 0.0, self.sample_rate);
        self.lp_mono.set_static(lp.0, lp.1, lp.2, lp.3, lp.4);
        self.hp_left.set_static(hp.0, hp.1, hp.2, hp.3, hp.4);
        self.hp_right.set_static(hp.0, hp.1, hp.2, hp.3, hp.4);
    }
}

impl Module for MonoBass {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "MonoBass",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::stereo("in")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::stereo("out")],
            per_axis_outputs: &[],
            realtime_params: &[ParameterTemplate {
                name: params::cutoff.as_str(),
                kind: ParameterKind::Float {
                    min: MIN_CUTOFF_HZ,
                    max: MAX_CUTOFF_HZ,
                    default: DEFAULT_CUTOFF_HZ,
                },
            }],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        let lp = compute_biquad_lowpass(DEFAULT_CUTOFF_HZ, 0.0, env.sample_rate);
        let hp = compute_biquad_highpass(DEFAULT_CUTOFF_HZ, 0.0, env.sample_rate);
        Ok(Self {
            instance_id,
            descriptor,
            sample_rate: env.sample_rate,
            cutoff_hz: DEFAULT_CUTOFF_HZ,
            in_stereo: StereoInput::default(),
            out_stereo: StereoOutput::default(),
            lp_mono: Lr4Filter::new(lp.0, lp.1, lp.2, lp.3, lp.4),
            hp_left: Lr4Filter::new(hp.0, hp.1, hp.2, hp.3, hp.4),
            hp_right: Lr4Filter::new(hp.0, hp.1, hp.2, hp.3, hp.4),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.cutoff_hz = p.get(params::cutoff).clamp(MIN_CUTOFF_HZ, MAX_CUTOFF_HZ);
        self.recompute_coeffs();
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }
    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_stereo = StereoInput::from_ports(inputs, 0);
        self.out_stereo = StereoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let (l, r) = pool.read_stereo(&self.in_stereo);
        let mono_in = (l + r) * 0.5;
        let mono_low = self.lp_mono.tick(mono_in);
        let l_high = self.hp_left.tick(l);
        let r_high = self.hp_right.tick(r);
        pool.write_stereo(&self.out_stereo, mono_low + l_high, mono_low + r_high);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::parameter_map::ParameterValue;
    use patches_core::test_support::ModuleHarness;

    const SR: f32 = 48_000.0;
    const ENV: AudioEnvironment = AudioEnvironment {
        sample_rate: SR,
        poly_voices: 1,
        periodic_update_interval: 32,
        hosted: false,
    };

    fn build(params: &[(&'static str, ParameterValue)]) -> ModuleHarness {
        ModuleHarness::build_full::<MonoBass>(
            params,
            ENV,
            patches_core::ModuleShape { channels: 0 },
        )
    }

    /// Run an N-sample sine at `freq_hz` through the harness, returning
    /// the RMS magnitude of the L output over the last quarter of the
    /// window (after transients).
    fn rms_left_at(h: &mut ModuleHarness, freq_hz: f32, l_amp: f32, r_amp: f32) -> f32 {
        let n = 8_192;
        let mut sum_sq = 0.0_f64;
        let mut counted = 0usize;
        for i in 0..n {
            let t = i as f32 / SR;
            let s = (std::f32::consts::TAU * freq_hz * t).sin();
            h.set_stereo("in", l_amp * s, r_amp * s);
            h.tick();
            let (l, _r) = h.read_stereo("out");
            if i >= n - n / 4 {
                sum_sq += (l as f64) * (l as f64);
                counted += 1;
            }
        }
        (sum_sq / counted as f64).sqrt() as f32
    }

    #[test]
    fn descriptor_shape() {
        let h = build(&[]);
        let desc = h.descriptor();
        assert_eq!(desc.inputs.len(), 1);
        assert_eq!(desc.outputs.len(), 1);
        assert_eq!(desc.inputs[0].name, "in");
        assert_eq!(desc.outputs[0].name, "out");
    }

    #[test]
    fn default_cutoff_passes_high_band_unchanged_per_channel() {
        // Well above 120 Hz, the LR4 sum-flat property means LP + HP
        // reconstruct unity magnitude. Differential L vs R should be
        // preserved on each side (no mono-summing of highs).
        let mut h = build(&[("cutoff", ParameterValue::Float(120.0))]);
        let freq = 5_000.0_f32;
        // Mono input (L = R): both outputs should equal the input.
        let rms_mono = rms_left_at(&mut h, freq, 1.0, 1.0);
        let expected_mono = 1.0_f32 / 2.0_f32.sqrt();
        assert!(
            (rms_mono - expected_mono).abs() < 0.02,
            "5kHz mono: RMS L = {rms_mono}, expected ≈ {expected_mono}"
        );

        let mut h = build(&[("cutoff", ParameterValue::Float(120.0))]);
        // Anti-phase (L = +1, R = -1): mono_low ≈ 0; L_high ≈ L.
        // RMS L ≈ amplitude / √2 ≈ 0.707.
        let rms_anti = rms_left_at(&mut h, freq, 1.0, -1.0);
        assert!(
            (rms_anti - expected_mono).abs() < 0.02,
            "5kHz anti-phase: RMS L = {rms_anti}, expected ≈ {expected_mono}"
        );
    }

    #[test]
    fn below_cutoff_anti_phase_is_summed_to_silence() {
        // Anti-phase content well below cutoff: mono_low = 0, L_high ≈ 0
        // (HPF kills it). L_out and R_out should both be near silent.
        let mut h = build(&[("cutoff", ParameterValue::Float(120.0))]);
        let freq = 20.0_f32;
        let n = 8_192;
        let mut max_l = 0.0_f32;
        for i in 0..n {
            let t = i as f32 / SR;
            let s = (std::f32::consts::TAU * freq * t).sin();
            h.set_stereo("in", s, -s);
            h.tick();
            if i >= n - n / 4 {
                let (l, _r) = h.read_stereo("out");
                max_l = max_l.max(l.abs());
            }
        }
        assert!(
            max_l < 0.05,
            "anti-phase 20 Hz should be summed away: max|L| = {max_l}"
        );
    }

    #[test]
    fn anti_phase_at_cutoff_is_minus_6db_each_side() {
        // Anti-phase input isolates the HP path (mono_low cancels to 0).
        // At the LR4 crossover frequency the HP magnitude is exactly
        // 0.5 (−6 dB) on each side — the sum-flat property's per-side
        // contribution.
        let cutoff = 200.0_f32;
        let mut h = build(&[("cutoff", ParameterValue::Float(cutoff))]);
        let rms = rms_left_at(&mut h, cutoff, 1.0, -1.0);
        // RMS of a 0.5-amplitude sine is 0.5/√2 ≈ 0.3535.
        let expected = 0.5_f32 / 2.0_f32.sqrt();
        assert!(
            (rms - expected).abs() < 0.02,
            "L_high at cutoff: RMS = {rms}, expected ≈ {expected}"
        );
    }

    #[test]
    fn cutoff_param_changes_crossover() {
        // With a very high cutoff (~1 kHz), a 200 Hz anti-phase signal
        // is now below the crossover and should be summed to silence.
        let mut h = build(&[("cutoff", ParameterValue::Float(1_500.0))]);
        let freq = 200.0_f32;
        let n = 8_192;
        let mut max_l = 0.0_f32;
        for i in 0..n {
            let t = i as f32 / SR;
            let s = (std::f32::consts::TAU * freq * t).sin();
            h.set_stereo("in", s, -s);
            h.tick();
            if i >= n - n / 4 {
                let (l, _r) = h.read_stereo("out");
                max_l = max_l.max(l.abs());
            }
        }
        assert!(
            max_l < 0.05,
            "200 Hz anti-phase under 1.5 kHz crossover should be silent: max|L| = {max_l}"
        );
    }
}
