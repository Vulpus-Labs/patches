use patches_core::{
    AudioEnvironment, BuildError, CablePool, CountAxis, InputPort, InstanceId, Module,
    ModuleDescriptor, ModuleDescriptorTemplate, MonoInput, OutputPort, ParameterKind,
    ParameterTemplate, PolyInput, PolyOutput, PortTemplate, StructuralParams,
};
use patches_core::module_params;
use patches_core::param_frame::ParamView;

use patches_dsp::hypersaw::{HyperSawCore, N_COPIES, N_VOICES};

use crate::common::frequency::{C0_FREQ, FMMode, PolyFrequencyConverter, PolyFrequencyChangeTracker};
use super::hypersaw::{base_increment, compute_detune, pack_voice};
use super::oscillator::OscFmType;

module_params! {
    PolyHyperSawParams {
        frequency: Float,
        fm_type:   Enum<OscFmType>,
        spread:    Float,
        density:   Float,
        mix:       Float,
    }
}

/// The 16-voice "supersaw": one detuned 9-saw ensemble per voice, driven by
/// per-voice `voct`/`fm`. The polyphonic counterpart of [`HyperSaw`](super::HyperSaw),
/// wrapping the same voice-batched [`HyperSawCore`] but filling all 16 lanes
/// (ADR 0078).
///
/// `spread`/`density`/`mix` CV are **mono (shared across voices)**: the 8 detune
/// ratios are computed once per period and reused for every voice, which is what
/// keeps the per-period cost down (ADR 0078 §3). Per-voice spread is deferred.
///
/// As with `HyperSaw`, **pitch and FM resolve at control rate** (the detune /
/// gain maths runs in `periodic_update`), FM is vibrato, and there is no
/// hard-sync and no phase modulation (ADR 0078 §6).
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `voct` | poly | V/oct pitch CV per voice |
/// | `fm` | poly | Frequency modulation per voice (control-rate vibrato) |
/// | `spread_cv` | mono | Adds to `spread`, shared across voices |
/// | `density_cv` | mono | Adds to `density`, shared across voices |
/// | `mix_cv` | mono | Adds to `mix`, shared across voices |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | poly | Per-voice detuned-saw ensemble (PolyBLEP anti-aliased) |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `frequency` | float | -4.0 -- 12.0 | `0.0` | Base pitch as V/oct offset from C0 |
/// | `fm_type` | enum | linear, logarithmic | `linear` | FM modulation mode |
/// | `spread` | float | 0.0 -- 1.0 | `0.3` | Detune width; `1.0` = ±50 cents at the outermost pair |
/// | `density` | float | 0.0 -- 1.0 | `1.0` | Side pairs faded in inner→outer (×4 pairs) |
/// | `mix` | float | 0.0 -- 1.0 | `0.7` | Centre↔side balance: `0` = clean centre saw, `1` = full stack |
pub struct PolyHyperSaw {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    core: HyperSawCore,
    freq_converter: PolyFrequencyConverter,
    freq_tracker: PolyFrequencyChangeTracker,
    spread: f32,
    density: f32,
    mix: f32,
    in_voct: PolyInput,
    in_fm: PolyInput,
    in_spread_cv: MonoInput,
    in_density_cv: MonoInput,
    in_mix_cv: MonoInput,
    out: PolyOutput,
    scratch: [f32; N_VOICES],
}

impl PolyHyperSaw {
    /// Recompute the full 16-voice batch: the shared detune/density/mix factors
    /// once, then a per-voice base increment + column fill (ADR 0078 §3–§4).
    fn recompute(&mut self, voct: &[f32; 16], fm: &[f32; 16], spread_cv: f32, density_cv: f32, mix_cv: f32) {
        // Shared across all voices — computed once, not per voice.
        let factors =
            compute_detune(self.spread + spread_cv, self.density + density_cv, self.mix + mix_cv);

        let mut inc = [[0u32; N_VOICES]; N_COPIES];
        let mut inv_inc = [[0.0f32; N_VOICES]; N_COPIES];
        let mut gain = [[0.0f32; N_VOICES]; N_COPIES];

        for v in 0..N_VOICES {
            let freq = self.freq_tracker.compute_modulated(v, voct[v], fm[v]);
            let (base_inc, inv_base) = base_increment(self.freq_converter.to_increment(freq));
            pack_voice(v, base_inc, inv_base, &factors, &mut inc, &mut inv_inc, &mut gain);
        }
        self.core.update(&inc, &inv_inc, &gain);
    }
}

impl Module for PolyHyperSaw {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "PolyHyperSaw",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[
                PortTemplate::poly("voct"),
                PortTemplate::poly("fm"),
                PortTemplate::mono("spread_cv"),
                PortTemplate::mono("density_cv"),
                PortTemplate::mono("mix_cv"),
            ],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::poly("out")],
            per_axis_outputs: &[],
            realtime_params: &[
                ParameterTemplate {
                    name: params::frequency.as_str(),
                    kind: ParameterKind::Float { min: -4.0, max: 12.0, default: 0.0 },
                },
                ParameterTemplate {
                    name: params::fm_type.as_str(),
                    kind: ParameterKind::Enum { variants: OscFmType::VARIANTS, default: "linear" },
                },
                ParameterTemplate {
                    name: params::spread.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.3 },
                },
                ParameterTemplate {
                    name: params::density.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 1.0 },
                },
                ParameterTemplate {
                    name: params::mix.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.7 },
                },
            ],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        let seed = instance_id.as_u64().wrapping_add(0x9E37_79B9_7F4A_7C15);
        Ok(Self {
            instance_id,
            descriptor,
            core: HyperSawCore::new(seed),
            freq_converter: PolyFrequencyConverter::new(audio_environment.sample_rate),
            freq_tracker: PolyFrequencyChangeTracker::new(C0_FREQ),
            spread: 0.3,
            density: 1.0,
            mix: 0.7,
            in_voct: PolyInput::default(),
            in_fm: PolyInput::default(),
            in_spread_cv: MonoInput::default(),
            in_density_cv: MonoInput::default(),
            in_mix_cv: MonoInput::default(),
            out: PolyOutput::default(),
            scratch: [0.0; N_VOICES],
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.freq_tracker.set_voct_offset(p.get(params::frequency));
        let fm_type: OscFmType = p.get(params::fm_type);
        let fm_mode = match fm_type {
            OscFmType::Linear => FMMode::Linear,
            OscFmType::Logarithmic => FMMode::Exponential,
        };
        self.freq_tracker.set_fm_mode(fm_mode);
        self.spread = p.get(params::spread);
        self.density = p.get(params::density);
        self.mix = p.get(params::mix);
        self.recompute(&[0.0; 16], &[0.0; 16], 0.0, 0.0, 0.0);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_voct = PolyInput::from_ports(inputs, 0);
        self.in_fm = PolyInput::from_ports(inputs, 1);
        self.in_spread_cv = MonoInput::from_ports(inputs, 2);
        self.in_density_cv = MonoInput::from_ports(inputs, 3);
        self.in_mix_cv = MonoInput::from_ports(inputs, 4);
        self.out = PolyOutput::from_ports(outputs, 0);

        self.freq_tracker.voct_modulating = self.in_voct.is_connected();
        self.freq_tracker.fm_modulating = self.in_fm.is_connected();
        self.recompute(&[0.0; 16], &[0.0; 16], 0.0, 0.0, 0.0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        self.core.process(&mut self.scratch);
        if self.out.is_connected() {
            pool.write_poly(&self.out, self.scratch);
        }
    }

    fn wants_periodic(&self) -> bool {
        true
    }

    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        let voct = if self.in_voct.is_connected() { pool.read_poly(&self.in_voct) } else { [0.0; 16] };
        let fm = if self.in_fm.is_connected() { pool.read_poly(&self.in_fm) } else { [0.0; 16] };
        let spread_cv =
            if self.in_spread_cv.is_connected() { pool.read_mono(&self.in_spread_cv) } else { 0.0 };
        let density_cv = if self.in_density_cv.is_connected() {
            pool.read_mono(&self.in_density_cv)
        } else {
            0.0
        };
        let mix_cv =
            if self.in_mix_cv.is_connected() { pool.read_mono(&self.in_mix_cv) } else { 0.0 };
        self.recompute(&voct, &fm, spread_cv, density_cv, mix_cv);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::frequency::C0_FREQ;
    use patches_core::test_support::{ModuleHarness, params};
    use patches_core::{AudioEnvironment, CableValue};

    fn env(sr: f32, voices: usize) -> AudioEnvironment {
        AudioEnvironment { sample_rate: sr, poly_voices: voices, periodic_update_interval: 32, hosted: false }
    }

    fn voct_for(freq: f32) -> f32 {
        (freq / C0_FREQ).log2()
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|&x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    fn dominant_bin(s: &[f32]) -> usize {
        let n = s.len();
        let fft = patches_dsp::RealPackedFft::new(n);
        let mut buf = s.to_vec();
        fft.forward(&mut buf);
        let half = n / 2;
        let mut best = 1;
        let mut best_mag = 0.0f32;
        for k in 1..half {
            let mag = (buf[2 * k] * buf[2 * k]) + (buf[2 * k + 1] * buf[2 * k + 1]);
            if mag > best_mag {
                best_mag = mag;
                best = k;
            }
        }
        best
    }

    #[test]
    fn voices_track_independent_voct() {
        // Voice 1 one octave above voice 0 → its fundamental bin is ~2× voice 0's.
        let sr = 48_000.0;
        let n = 16_384;
        let mut h = ModuleHarness::build_with_env::<PolyHyperSaw>(
            params!["frequency" => voct_for(220.0), "spread" => 0.3, "mix" => 0.7],
            env(sr, 2),
        );
        h.disconnect_input("fm");
        h.disconnect_input("spread_cv");
        h.disconnect_input("density_cv");
        h.disconnect_input("mix_cv");
        let mut voct = [voct_for(220.0); 16];
        voct[1] = voct_for(440.0);
        h.set_poly("voct", voct);

        let frames = h.run_poly(n, "out");
        let v0: Vec<f32> = frames.iter().map(|f| f[0]).collect();
        let v1: Vec<f32> = frames.iter().map(|f| f[1]).collect();
        let b0 = dominant_bin(&v0);
        let b1 = dominant_bin(&v1);
        let ratio = b1 as f32 / b0 as f32;
        assert!((ratio - 2.0).abs() < 0.2, "voice 1 should be an octave up: {b0} → {b1}");
    }

    #[test]
    fn parity_with_mono_for_single_voice() {
        // Driving one voice of PolyHyperSaw with the same params as HyperSaw must
        // give the same timbre. The two instances get different phase seeds
        // (`InstanceId::next()`), so individual samples differ; but the gain
        // structure is identical, so the long-run RMS (cross-terms of detuned
        // copies average to zero) and the pitch converge. Use a multi-second
        // power-of-two window so the sub-Hz beating washes out.
        use super::super::HyperSaw;
        let sr = 48_000.0;
        let n = 1 << 17; // ~2.7 s
        let p = params!["frequency" => voct_for(220.0), "spread" => 0.6, "density" => 0.8, "mix" => 0.7];

        let mut mono = ModuleHarness::build_with_env::<HyperSaw>(p, env(sr, 1));
        mono.disconnect_all_inputs();
        let mono_out = mono.run_mono(n, "out");

        let mut poly = ModuleHarness::build_with_env::<PolyHyperSaw>(p, env(sr, 1));
        poly.disconnect_all_inputs();
        let poly_out: Vec<f32> = poly.run_poly(n, "out").iter().map(|f| f[0]).collect();

        assert_eq!(dominant_bin(&mono_out), dominant_bin(&poly_out), "pitch parity");
        assert!(
            (rms(&mono_out) - rms(&poly_out)).abs() < 0.02,
            "level parity: mono {} vs poly {}",
            rms(&mono_out),
            rms(&poly_out)
        );
    }

    #[test]
    fn shared_cv_affects_all_voices() {
        // spread_cv is mono/shared: raising it widens every voice's detune
        // identically. Verify all driven voices respond (RMS finite, bounded)
        // and the detune ratio on the core is the same column-to-column.
        let sr = 48_000.0;
        let mut h = ModuleHarness::build_with_env::<PolyHyperSaw>(
            params!["frequency" => voct_for(330.0), "spread" => 0.0, "mix" => 0.7],
            env(sr, 4),
        );
        h.disconnect_input("fm");
        h.disconnect_input("density_cv");
        h.disconnect_input("mix_cv");
        h.set_poly("voct", [voct_for(330.0); 16]);
        h.set_mono("spread_cv", 1.0); // full spread via shared CV
        h.tick();

        let hs = h.as_any().downcast_ref::<PolyHyperSaw>().expect("PolyHyperSaw");
        // Outermost above side (copy 8) vs centre (copy 0): same ratio for every voice.
        let r0 = hs.core.increment(8, 0) as f64 / hs.core.increment(0, 0) as f64;
        for v in 1..4 {
            let rv = hs.core.increment(8, v) as f64 / hs.core.increment(0, v) as f64;
            assert!((rv - r0).abs() < 1e-6, "voice {v} detune {rv} != voice 0 {r0}");
        }
        assert!((r0 - 2.0f64.powf(1.0 / 24.0)).abs() < 1e-3, "shared spread_cv=1 → ±50 cents");
    }

    #[test]
    fn output_bounded_and_finite() {
        let sr = 48_000.0;
        let mut h = ModuleHarness::build_with_env::<PolyHyperSaw>(
            params!["frequency" => voct_for(110.0), "spread" => 1.0, "density" => 1.0, "mix" => 1.0],
            env(sr, 16),
        );
        h.disconnect_input("fm");
        h.disconnect_input("spread_cv");
        h.disconnect_input("density_cv");
        h.disconnect_input("mix_cv");
        h.set_poly("voct", std::array::from_fn(|i| voct_for(110.0) + i as f32 * 0.05));
        let frames = h.run_poly(sr as usize, "out");
        for f in &frames {
            for &x in f.iter() {
                assert!(x.is_finite() && x.abs() <= 1.0001, "out of range: {x}");
            }
        }
    }

    #[test]
    fn disconnected_output_not_written() {
        let mut h = ModuleHarness::build_with_env::<PolyHyperSaw>(
            params!["frequency" => voct_for(220.0)],
            env(48_000.0, 4),
        );
        h.disconnect_all_inputs();
        h.disconnect_all_outputs();
        h.init_pool(CableValue::poly([99.0; 16]));
        h.tick();
        for (i, &v) in h.read_poly("out").iter().take(4).enumerate() {
            assert_eq!(99.0_f32, v, "out voice {i} written despite disconnected");
        }
    }
}
