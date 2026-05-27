use patches_core::{
    AudioEnvironment, BuildError, CablePool, CountAxis, InputPort, InstanceId, Module,
    ModuleDescriptor, ModuleDescriptorTemplate, MonoInput, MonoOutput, OutputPort, ParameterKind,
    ParameterTemplate, PortTemplate, StructuralParams,
};
use patches_core::module_params;
use patches_core::param_frame::ParamView;

use patches_dsp::hypersaw::{HyperSawCore, N_COPIES, N_PAIRS, N_VOICES};

use crate::common::approximate::fast_exp2;
use crate::common::frequency::{C0_FREQ, FMMode, MonoFrequencyConverter, MonoFrequencyChangeTracker};
use super::oscillator::OscFmType;

module_params! {
    HyperSawParams {
        frequency: Float,
        fm_type:   Enum<OscFmType>,
        spread:    Float,
        density:   Float,
        mix:       Float,
    }
}

/// Per-pair detune magnitude multipliers, inner→outer (JP-8000-ish nonlinear
/// spacing). `M[3] = 1.0` puts the outermost pair at the full spread (ADR 0078 §3).
const PAIR_MAGNITUDE: [f32; N_PAIRS] = [0.18, 0.43, 0.71, 1.00];

/// Outermost detune at `spread = 1`: ±1/24 octave = ±50 cents (ADR 0078 §3).
const FULL_SPREAD_OCT: f32 = 1.0 / 24.0;

/// Nyquist guard on the per-sample increment, as a fraction of 2³².
const MAX_INC_FRAC: f32 = 0.4999;

/// The voice-shared half of the control-rate maths (ADR 0078 §3–§4): the 8
/// detune ratios, the per-pair density fade, and the centre/side mix weights.
/// Computed once per period and reused across every voice — this sharing is why
/// `spread`/`density`/`mix` CV stay mono (poly spread is deferred).
pub(super) struct DetuneFactors {
    /// Above-side detune ratio per pair (below side = reciprocal).
    r: [f32; N_PAIRS],
    inv_r: [f32; N_PAIRS],
    /// Per-pair fade gain in `[0, 1]` (density).
    pair_gain: [f32; N_PAIRS],
    /// Centre copy weight.
    w_centre: f32,
    /// Per-side weight before the pair fade (loudness-normalised).
    w_side: f32,
}

/// Compute the voice-shared detune/density/mix factors from already-combined
/// (param + CV) `spread`, `density`, and `mix` values.
pub(super) fn compute_detune(spread: f32, density: f32, mix: f32) -> DetuneFactors {
    let s = spread.clamp(0.0, 1.0);
    let mut r = [1.0f32; N_PAIRS];
    let mut inv_r = [1.0f32; N_PAIRS];
    for i in 0..N_PAIRS {
        let off = PAIR_MAGNITUDE[i] * s * FULL_SPREAD_OCT;
        r[i] = fast_exp2(off);
        inv_r[i] = fast_exp2(-off);
    }

    let d = density.clamp(0.0, 1.0) * N_PAIRS as f32;
    let mut pair_gain = [0.0f32; N_PAIRS];
    let mut eff = 0.0f32;
    for (i, slot) in pair_gain.iter_mut().enumerate() {
        let g = (d - i as f32).clamp(0.0, 1.0);
        *slot = g;
        eff += 2.0 * g;
    }

    // mix = 0: clean centre saw. mix = 1: full stack (centre still present).
    // Dividing by (1 + mix) bounds the summed output to [-1, 1] (ADR 0078 §4).
    let m = mix.clamp(0.0, 1.0);
    let norm = 1.0 + m;
    let w_centre = 1.0 / norm;
    let w_side = m / (norm * eff.max(f32::MIN_POSITIVE));

    DetuneFactors { r, inv_r, pair_gain, w_centre, w_side }
}

/// Fill column `v` of the core's per-copy arrays for one voice at base
/// increment `base_inc` (reciprocal `inv_base`), applying the shared `factors`.
#[allow(clippy::too_many_arguments)]
pub(super) fn pack_voice(
    v: usize,
    base_inc: u32,
    inv_base: f32,
    factors: &DetuneFactors,
    inc: &mut [[u32; N_VOICES]; N_COPIES],
    inv_inc: &mut [[f32; N_VOICES]; N_COPIES],
    gain: &mut [[f32; N_VOICES]; N_COPIES],
) {
    // Copy 0 = centre (ratio 1).
    inc[0][v] = base_inc;
    inv_inc[0][v] = inv_base;
    gain[0][v] = factors.w_centre;

    // Copies 1..=8: pair i occupies (2i+1) = below, (2i+2) = above.
    for i in 0..N_PAIRS {
        let below = 2 * i + 1;
        let above = 2 * i + 2;
        let g = factors.w_side * factors.pair_gain[i];
        // Below side: pitch down → ratio 1/r, reciprocal r.
        inc[below][v] = ((base_inc as f32) * factors.inv_r[i]) as u32;
        inv_inc[below][v] = inv_base * factors.r[i];
        gain[below][v] = g;
        // Above side: pitch up → ratio r, reciprocal 1/r.
        inc[above][v] = ((base_inc as f32) * factors.r[i]) as u32;
        inv_inc[above][v] = inv_base * factors.inv_r[i];
        gain[above][v] = g;
    }
}

/// Q32 increment + reciprocal from a normalised per-sample increment
/// (`frequency / sample_rate`, i.e. cycles per sample).
pub(super) fn base_increment(inc_frac: f32) -> (u32, f32) {
    let inc_frac = inc_frac.clamp(0.0, MAX_INC_FRAC);
    let base_inc = (inc_frac * 4_294_967_296.0) as u32;
    let inv_base = if base_inc > 0 { 1.0 / base_inc as f32 } else { 0.0 };
    (base_inc, inv_base)
}

/// A "supersaw": a stack of 9 detuned sawtooth copies (1 centre + 8 sides)
/// summed to one voice, whose slow inter-copy beating gives the wide, animated
/// lead/pad character.
///
/// Built on the voice-batched [`HyperSawCore`] (ADR 0078): the kernel runs a
/// fixed 16-voice batch and this mono module reads lane 0, leaving 15 lanes
/// idle. All frequency maths — spread → detune ratios, pitch/FM → base
/// increment, density → per-copy gains, mix → centre/side balance — is computed
/// at **control rate** in `periodic_update` and pushed into the core; per-sample
/// `process` only advances phases and sums.
///
/// Consequently **pitch and FM resolve at control rate** (`periodic_update_interval`
/// samples), unlike `Osc`/`PolyOsc` which recompute per sample. FM is therefore
/// vibrato, not audio-rate modulation. There is **no hard-sync and no phase
/// modulation** (ADR 0078 §6): syncing would re-correlate the copies and
/// collapse the beating that is the module's reason to exist.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `voct` | mono | V/oct pitch CV added to the base frequency |
/// | `fm` | mono | Frequency modulation (control-rate vibrato; `fm_type` selects mode) |
/// | `spread_cv` | mono | Adds to `spread` (detune width) |
/// | `density_cv` | mono | Adds to `density` (number of active side pairs) |
/// | `mix_cv` | mono | Adds to `mix` (centre↔side balance) |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | mono | Summed detuned-saw ensemble (PolyBLEP anti-aliased) |
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
pub struct HyperSaw {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    core: HyperSawCore,
    freq_converter: MonoFrequencyConverter,
    freq_tracker: MonoFrequencyChangeTracker,
    // Parameter values (control-rate).
    spread: f32,
    density: f32,
    mix: f32,
    // Inputs (mono).
    in_voct: MonoInput,
    in_fm: MonoInput,
    in_spread_cv: MonoInput,
    in_density_cv: MonoInput,
    in_mix_cv: MonoInput,
    // Output (mono).
    out: MonoOutput,
    // Reusable per-sample output scratch (15 lanes idle for mono).
    scratch: [f32; N_VOICES],
}

impl HyperSaw {
    /// Recompute the core's per-copy increments, reciprocals, and gains from the
    /// current parameters plus the supplied CV values. The whole detune /
    /// density / mix maths (ADR 0078 §3–§4) lives here; it runs at control rate.
    fn recompute(&mut self, voct: f32, fm: f32, spread_cv: f32, density_cv: f32, mix_cv: f32) {
        let factors =
            compute_detune(self.spread + spread_cv, self.density + density_cv, self.mix + mix_cv);
        let freq = self.freq_tracker.compute_modulated(voct, fm);
        let (base_inc, inv_base) = base_increment(self.freq_converter.to_increment(freq));

        let mut inc = [[0u32; N_VOICES]; N_COPIES];
        let mut inv_inc = [[0.0f32; N_VOICES]; N_COPIES];
        let mut gain = [[0.0f32; N_VOICES]; N_COPIES];
        pack_voice(0, base_inc, inv_base, &factors, &mut inc, &mut inv_inc, &mut gain);
        self.core.update(&inc, &inv_inc, &gain);
    }
}

impl Module for HyperSaw {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "HyperSaw",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[
                PortTemplate::mono("voct"),
                PortTemplate::mono("fm"),
                PortTemplate::mono("spread_cv"),
                PortTemplate::mono("density_cv"),
                PortTemplate::mono("mix_cv"),
            ],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::mono("out")],
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
        // Decorrelate this instance's start phases from any other (ADR 0078 §5).
        let seed = instance_id.as_u64().wrapping_add(0x9E37_79B9_7F4A_7C15);
        Ok(Self {
            instance_id,
            descriptor,
            core: HyperSawCore::new(seed),
            freq_converter: MonoFrequencyConverter::new(audio_environment.sample_rate),
            freq_tracker: MonoFrequencyChangeTracker::new(C0_FREQ),
            spread: 0.3,
            density: 1.0,
            mix: 0.7,
            in_voct: MonoInput::default(),
            in_fm: MonoInput::default(),
            in_spread_cv: MonoInput::default(),
            in_density_cv: MonoInput::default(),
            in_mix_cv: MonoInput::default(),
            out: MonoOutput::default(),
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
        // Refresh the core with no CV (connectivity may not be set yet; the
        // first periodic_update refines it once the pool is available).
        self.recompute(0.0, 0.0, 0.0, 0.0, 0.0);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_voct = MonoInput::from_ports(inputs, 0);
        self.in_fm = MonoInput::from_ports(inputs, 1);
        self.in_spread_cv = MonoInput::from_ports(inputs, 2);
        self.in_density_cv = MonoInput::from_ports(inputs, 3);
        self.in_mix_cv = MonoInput::from_ports(inputs, 4);
        self.out = MonoOutput::from_ports(outputs, 0);

        self.freq_tracker.voct_modulating = self.in_voct.is_connected();
        self.freq_tracker.fm_modulating = self.in_fm.is_connected();
        // Seed the core so the first samples (before the first periodic tick)
        // are not silent.
        self.recompute(0.0, 0.0, 0.0, 0.0, 0.0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        self.core.process(&mut self.scratch);
        if self.out.is_connected() {
            pool.write_mono(&self.out, self.scratch[0]);
        }
    }

    fn wants_periodic(&self) -> bool {
        true
    }

    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        let voct = if self.in_voct.is_connected() { pool.read_mono(&self.in_voct) } else { 0.0 };
        let fm = if self.in_fm.is_connected() { pool.read_mono(&self.in_fm) } else { 0.0 };
        let spread_cv =
            if self.in_spread_cv.is_connected() { pool.read_mono(&self.in_spread_cv) } else { 0.0 };
        let density_cv = if self.in_density_cv.is_connected() {
            pool.read_mono(&self.in_density_cv)
        } else {
            0.0
        };
        let mix_cv =
            if self.in_mix_cv.is_connected() { pool.read_mono(&self.in_mix_cv) } else { 0.0 };
        self.recompute(voct, fm, spread_cv, density_cv, mix_cv);
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

    fn env(sr: f32) -> AudioEnvironment {
        AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32, hosted: false }
    }

    fn voct_for(freq: f32) -> f32 {
        (freq / C0_FREQ).log2()
    }

    /// Build a HyperSaw at the given params, all CV inputs disconnected.
    fn build(freq: f32, spread: f32, density: f32, mix: f32, sr: f32) -> ModuleHarness {
        let mut h = ModuleHarness::build_with_env::<HyperSaw>(
            params![
                "frequency" => voct_for(freq),
                "spread" => spread,
                "density" => density,
                "mix" => mix
            ],
            env(sr),
        );
        h.disconnect_all_inputs();
        h
    }

    /// Peak-per-block envelope variance — a proxy for inter-copy beating.
    fn env_variance(s: &[f32]) -> f32 {
        let block = 1024;
        let peaks: Vec<f32> = s
            .chunks(block)
            .map(|c| c.iter().fold(0.0f32, |m, &x| m.max(x.abs())))
            .collect();
        let mean = peaks.iter().sum::<f32>() / peaks.len() as f32;
        peaks.iter().map(|&p| (p - mean) * (p - mean)).sum::<f32>() / peaks.len() as f32
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
    fn spread_zero_is_unison_spread_one_beats() {
        // spread=0: all copies share pitch → static sum, little beating.
        // spread=1: copies detuned → slow amplitude beating.
        let sr = 48_000.0;
        let n = 48_000;
        let unison = build(220.0, 0.0, 1.0, 0.7, sr).run_mono(n, "out");
        let wide = build(220.0, 1.0, 1.0, 0.7, sr).run_mono(n, "out");
        assert!(unison.iter().all(|x| x.is_finite()));
        assert!(
            env_variance(&wide) > env_variance(&unison) * 4.0,
            "spread=1 should beat far more than spread=0: {} vs {}",
            env_variance(&wide),
            env_variance(&unison)
        );
    }

    #[test]
    fn spread_one_outermost_is_fifty_cents() {
        // The outermost side pair sits at ±50 cents = ratio 2^(±1/24). Read the
        // packed increments back from the core to check the spread maths.
        let mut h = build(1000.0, 1.0, 1.0, 0.7, 48_000.0);
        h.tick(); // fire periodic_update so the core is populated
        let hs = h.as_any().downcast_ref::<HyperSaw>().expect("HyperSaw");
        let centre = hs.core.increment(0, 0) as f64;
        let above = hs.core.increment(8, 0) as f64; // outermost above side
        let below = hs.core.increment(7, 0) as f64; // outermost below side
        let expected = 2.0f64.powf(1.0 / 24.0);
        assert!(
            (above / centre - expected).abs() < 1e-3,
            "outermost above detune {} != 2^(1/24) {expected}",
            above / centre
        );
        assert!(
            (centre / below - expected).abs() < 1e-3,
            "outermost below detune {} != 2^(1/24)",
            centre / below
        );
    }

    #[test]
    fn mix_zero_is_clean_centre_saw() {
        // mix=0 silences every side; output is a single full-amplitude saw.
        let sr = 48_000.0;
        let n = 48_000;
        let s = build(220.0, 1.0, 1.0, 0.0, sr).run_mono(n, "out");
        // A saw's RMS is 1/sqrt(3) ≈ 0.577; sides off ⇒ no beating.
        assert!((rms(&s) - 0.577).abs() < 0.05, "centre-only RMS {} != saw", rms(&s));
        let beating = env_variance(&s);
        let wide = build(220.0, 1.0, 1.0, 0.7, sr).run_mono(n, "out");
        assert!(beating < env_variance(&wide) * 0.25, "mix=0 must not beat");
    }

    #[test]
    fn density_sweep_holds_level_and_stays_bounded() {
        // Loudness-normalised: RMS should stay in a tight band as pairs fade in,
        // and the output never clips.
        let sr = 48_000.0;
        let n = 24_000;
        let mut rmss = Vec::new();
        for &d in &[0.1f32, 0.3, 0.5, 0.7, 1.0] {
            let s = build(220.0, 0.6, d, 0.7, sr).run_mono(n, "out");
            assert!(s.iter().all(|x| x.is_finite() && x.abs() <= 1.0001), "clipped at density={d}");
            rmss.push(rms(&s));
        }
        let mean = rmss.iter().sum::<f32>() / rmss.len() as f32;
        for (i, &r) in rmss.iter().enumerate() {
            assert!(
                (r - mean).abs() < mean * 0.5,
                "density sweep RMS jump at {i}: {r} vs mean {mean}"
            );
        }
    }

    #[test]
    fn fm_logarithmic_shifts_pitch_one_octave() {
        // fm_type=logarithmic, fm=+1.0 → +1 octave → fundamental doubles.
        // (Control-rate vibrato — the shift is sampled in periodic_update.)
        let sr = 48_000.0;
        let n = 16_384;
        let freq = 300.0;

        let mut base = ModuleHarness::build_with_env::<HyperSaw>(
            params![
                "frequency" => voct_for(freq),
                "fm_type" => OscFmType::Logarithmic,
                "spread" => 0.0,
                "density" => 0.0,
                "mix" => 0.0
            ],
            env(sr),
        );
        base.disconnect_all_inputs();
        let no_fm = base.run_mono(n, "out");

        let mut up = ModuleHarness::build_with_env::<HyperSaw>(
            params![
                "frequency" => voct_for(freq),
                "fm_type" => OscFmType::Logarithmic,
                "spread" => 0.0,
                "density" => 0.0,
                "mix" => 0.0
            ],
            env(sr),
        );
        up.disconnect_inputs(&["spread_cv", "density_cv", "mix_cv", "voct"]);
        up.set_mono("fm", 1.0);
        let fm_up = up.run_mono(n, "out");

        let b0 = dominant_bin(&no_fm);
        let b1 = dominant_bin(&fm_up);
        let ratio = b1 as f32 / b0 as f32;
        assert!(
            (ratio - 2.0).abs() < 0.15,
            "logarithmic fm=+1 should double pitch: bins {b0} → {b1} (ratio {ratio})"
        );
    }

    #[test]
    fn disconnected_output_not_written() {
        let mut h = build(220.0, 0.5, 1.0, 0.7, 48_000.0);
        h.disconnect_all_outputs();
        h.init_pool(CableValue::mono(99.0));
        h.tick();
        assert_eq!(99.0_f32, h.read_mono("out"), "out written despite disconnected");
    }
}
