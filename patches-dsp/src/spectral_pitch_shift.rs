//! Phase-vocoder spectral pitch shifter (Laroche & Dolson region-based).
//!
//! Shifts pitch by identifying spectral peaks, partitioning bins into regions
//! of influence, and shifting entire regions as blocks with a single complex
//! rotation per region.  This preserves inter-bin phase coherence (identity
//! phase locking) without requiring per-bin phase interpolation.
//!
//! Based on: Laroche & Dolson, "New Phase-Vocoder Techniques for
//! Pitch-Shifting, Harmonizing and Other Exotic Effects" (1999) and
//! US Patent US6549884B1 (expired 2019).
//!
//! Operates on the packed real FFT format produced by [`RealPackedFft`]:
//!
//! ```text
//!   [0]     = DC (real)
//!   [1]     = Nyquist (real)
//!   [2k]    = bin k real,  k = 1 .. N/2-1
//!   [2k+1]  = bin k imag
//! ```
//!
//! Call [`SpectralPitchShifter::transform`] on each windowed, FFT'd frame
//! between the forward and inverse FFT steps.

use std::f32::consts::PI;

const TWO_PI: f32 = 2.0 * PI;

/// Phase-vocoder pitch shifter for packed real FFT spectra.
///
/// Construct once per window/hop configuration. Call [`transform`](Self::transform)
/// on each frame's packed spectrum (between forward and inverse FFT).
///
/// # Parameters (set between frames)
///
/// - **shift_ratio**: frequency multiplier (2.0 = octave up, 0.5 = octave down).
///   Use [`set_shift_semitones`](Self::set_shift_semitones) for musical intervals.
/// - **mix**: dry/wet blend (0.0 = original, 1.0 = fully shifted).
/// - **preserve_formants**: when true, applies spectral envelope correction to
///   avoid the "chipmunk effect" on upward shifts.
pub struct SpectralPitchShifter {
    /// Number of frequency bins: N/2 + 1 (DC through Nyquist inclusive).
    half_n: usize,
    /// `2π · hop_size / window_size` — expected phase advance per bin per hop.
    phase_scale: f32,

    // Parameters
    shift_ratio: f32,
    mix: f32,
    preserve_formants: bool,
    mono: bool,

    // Shared phase tracking
    prev_phase: Vec<f32>,

    // Region-based (mono) state
    synth_phase: Vec<f32>,
    peaks: Vec<usize>,

    // Per-bin (poly) state
    phase_deviation: Vec<f32>,
    phase_accumulator: Vec<f32>,

    // Shared working buffers (pre-allocated, avoid per-frame allocation)
    analysis_re: Vec<f32>,
    analysis_im: Vec<f32>,
    shifted_re: Vec<f32>,
    shifted_im: Vec<f32>,
    magnitude: Vec<f32>,
    phase: Vec<f32>,
    shifted_mag: Vec<f32>,
    original_spectrum: Vec<f32>,
    envelope_buf: Vec<f32>,
    shifted_envelope_buf: Vec<f32>,
}

impl SpectralPitchShifter {
    /// Create a pitch shifter for the given window and hop sizes.
    ///
    /// `window_size` must match the [`RealPackedFft`] length. `hop_size` is
    /// typically `window_size / overlap_factor`.
    pub fn new(window_size: usize, hop_size: usize) -> Self {
        let half_n = (window_size >> 1) + 1;
        Self {
            half_n,
            phase_scale: TWO_PI * hop_size as f32 / window_size as f32,
            shift_ratio: 1.0,
            mix: 1.0,
            preserve_formants: false,
            mono: false,
            prev_phase: vec![0.0; half_n],
            synth_phase: vec![0.0; half_n],
            peaks: Vec::with_capacity(half_n / 4),
            phase_deviation: vec![0.0; half_n],
            phase_accumulator: vec![0.0; half_n],
            analysis_re: vec![0.0; half_n],
            analysis_im: vec![0.0; half_n],
            shifted_re: vec![0.0; half_n],
            shifted_im: vec![0.0; half_n],
            magnitude: vec![0.0; half_n],
            phase: vec![0.0; half_n],
            shifted_mag: vec![0.0; half_n],
            original_spectrum: vec![0.0; window_size],
            envelope_buf: vec![0.0; half_n],
            shifted_envelope_buf: vec![0.0; half_n],
        }
    }

    /// Set pitch shift in semitones (+12 = octave up, -12 = octave down, +7 = perfect fifth).
    pub fn set_shift_semitones(&mut self, semitones: f32) {
        self.shift_ratio = (2.0f32).powf(semitones / 12.0);
    }

    /// Set pitch shift as a raw frequency ratio (2.0 = octave up, 0.5 = octave down).
    pub fn set_shift_ratio(&mut self, ratio: f32) {
        self.shift_ratio = ratio;
    }

    /// Set dry/wet mix. Clamped to `[0, 1]`.
    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    /// Enable or disable formant preservation.
    pub fn set_preserve_formants(&mut self, preserve: bool) {
        self.preserve_formants = preserve;
    }

    /// Enable mono mode (region-based Laroche & Dolson shifting).
    ///
    /// - **mono = true**: shifts entire spectral regions as blocks with a
    ///   single complex rotation per peak.  Best for monophonic input
    ///   (eliminates phasiness artefacts).
    /// - **mono = false** (default): per-bin resampling with independent
    ///   phase propagation.  Better for polyphonic input where dense peaks
    ///   make region boundaries audible.
    pub fn set_mono(&mut self, mono: bool) {
        self.mono = mono;
    }

    /// Reset phase tracking state. Call when starting a new stream.
    pub fn reset(&mut self) {
        self.prev_phase.fill(0.0);
        self.synth_phase.fill(0.0);
        self.phase_accumulator.fill(0.0);
    }

    /// Transform a packed spectrum in-place.
    ///
    /// `spectrum` must have length `window_size` (the value passed to [`new`](Self::new))
    /// and contain the output of [`RealPackedFft::forward`].
    pub fn transform(&mut self, spectrum: &mut [f32]) {
        let half_n = self.half_n;
        let shift_ratio = self.shift_ratio;

        // Save original for mixing.
        if self.mix < 1.0 {
            self.original_spectrum[..spectrum.len()].copy_from_slice(spectrum);
        }

        // 1. Unpack packed real FFT into separate real/imag arrays.
        self.unpack(spectrum);

        // 2. Extract magnitude and phase.
        for k in 0..half_n {
            self.magnitude[k] = self.analysis_re[k].hypot(self.analysis_im[k]);
            self.phase[k] = self.analysis_im[k].atan2(self.analysis_re[k]);
        }

        // 3. Spectral envelope for formant preservation.
        if self.preserve_formants {
            spectral_envelope_into(&self.magnitude, half_n, &mut self.envelope_buf);
        }

        // 4. Shift — dispatch to mono (region-based) or poly (per-bin).
        //    Both paths produce output in shifted_re / shifted_im.
        if self.mono {
            self.shift_region_based();
        } else {
            self.shift_per_bin();
        }

        // 5. Update previous phase for next frame.
        self.prev_phase.copy_from_slice(&self.phase);

        // 6. Formant correction.
        if self.preserve_formants {
            self.apply_formant_correction(shift_ratio);
        }

        // 7. Pack back into packed real FFT format.
        self.pack(spectrum);

        // 8. Complex-domain dry/wet mix.
        if self.mix < 1.0 {
            mix_complex_spectra(&self.original_spectrum, spectrum, self.mix, half_n);
        }
    }

    /// Region-based shifting (Laroche & Dolson).
    ///
    /// Identifies spectral peaks, partitions bins into regions, and shifts
    /// each region as a block with a single complex rotation.  Best for
    /// monophonic input.
    fn shift_region_based(&mut self) {
        let half_n = self.half_n;
        let shift_ratio = self.shift_ratio;

        self.detect_peaks();

        self.shifted_re.fill(0.0);
        self.shifted_im.fill(0.0);

        let num_peaks = self.peaks.len();
        for i in 0..num_peaks {
            let p = self.peaks[i];

            let left = if i == 0 {
                0
            } else {
                (self.peaks[i - 1] + p).div_ceil(2)
            };
            let right = if i == num_peaks - 1 {
                half_n
            } else {
                (p + self.peaks[i + 1]).div_ceil(2)
            };

            let target = (p as f32 * shift_ratio).round() as isize;
            let delta = target - p as isize;

            let inst_freq = self.phase_scale * p as f32
                + principal_argument(
                    self.phase[p] - self.prev_phase[p] - self.phase_scale * p as f32,
                );

            let omega_output = inst_freq * shift_ratio;

            let target_usize = target.clamp(0, half_n as isize - 1) as usize;
            self.synth_phase[target_usize] += omega_output;
            let phi_s = self.synth_phase[target_usize];
            self.synth_phase[target_usize] = principal_argument(phi_s);

            let rotation = phi_s - self.phase[p];
            let (sin_r, cos_r) = rotation.sin_cos();

            for k in left..right {
                let target_k = k as isize + delta;
                if target_k >= 0 && (target_k as usize) < half_n {
                    let tk = target_k as usize;
                    let re = self.analysis_re[k];
                    let im = self.analysis_im[k];
                    self.shifted_re[tk] += cos_r * re - sin_r * im;
                    self.shifted_im[tk] += sin_r * re + cos_r * im;
                }
            }
        }
    }

    /// Per-bin resampling (standard phase vocoder).
    ///
    /// Each output bin independently reads from a fractional source position,
    /// interpolating magnitude and phase deviation.  No phase locking — each
    /// bin's phase propagates independently.  Better for polyphonic input.
    fn shift_per_bin(&mut self) {
        let half_n = self.half_n;
        let shift_ratio = self.shift_ratio;

        // Phase deviations.
        for k in 0..half_n {
            let expected = self.prev_phase[k] + self.phase_scale * k as f32;
            self.phase_deviation[k] = principal_argument(self.phase[k] - expected);
        }

        // Resample bins with interpolation.
        for k in 0..half_n {
            let source = k as f32 / shift_ratio;
            if source >= (half_n - 1) as f32 {
                self.shifted_re[k] = 0.0;
                self.shifted_im[k] = 0.0;
                self.phase_accumulator[k] = 0.0;
            } else {
                let mag = cubic(&self.magnitude, source);
                let interp_dev = cubic(&self.phase_deviation, source);
                let advance = self.phase_scale * k as f32;
                let ph =
                    self.phase_accumulator[k] + advance + shift_ratio * interp_dev;
                self.phase_accumulator[k] = principal_argument(ph);
                let (sin_p, cos_p) = ph.sin_cos();
                self.shifted_re[k] = mag * cos_p;
                self.shifted_im[k] = mag * sin_p;
            }
        }
    }

    // -- internal helpers ----------------------------------------------------

    /// Unpack packed real FFT format into separate real/imag arrays.
    fn unpack(&mut self, spectrum: &[f32]) {
        self.analysis_re[0] = spectrum[0];
        self.analysis_im[0] = 0.0;

        let last = self.half_n - 1;
        self.analysis_re[last] = spectrum[1];
        self.analysis_im[last] = 0.0;

        for k in 1..last {
            self.analysis_re[k] = spectrum[2 * k];
            self.analysis_im[k] = spectrum[2 * k + 1];
        }
    }

    /// Pack shifted real/imag arrays back into packed real FFT format.
    fn pack(&self, spectrum: &mut [f32]) {
        spectrum[0] = self.shifted_re[0];
        spectrum[1] = self.shifted_re[self.half_n - 1];

        let last = self.half_n - 1;
        for k in 1..last {
            spectrum[2 * k] = self.shifted_re[k];
            spectrum[2 * k + 1] = self.shifted_im[k];
        }
    }

    /// Detect spectral peaks.  Interior bins use a 4-neighbour criterion
    /// (must exceed 2 bins on each side); edge bins use 2-neighbour.
    fn detect_peaks(&mut self) {
        let half_n = self.half_n;
        self.peaks.clear();

        for k in 1..half_n - 1 {
            let m = self.magnitude[k];
            // Must exceed immediate neighbours.
            if m <= self.magnitude[k - 1] || m <= self.magnitude[k + 1] {
                continue;
            }
            // Interior bins: also check second neighbours.
            if k >= 2 && m <= self.magnitude[k - 2] {
                continue;
            }
            if k + 2 < half_n && m <= self.magnitude[k + 2] {
                continue;
            }
            self.peaks.push(k);
        }
    }

    /// Apply formant correction: rescale shifted magnitudes so the spectral
    /// envelope matches the original, then adjust complex bins accordingly.
    fn apply_formant_correction(&mut self, shift_ratio: f32) {
        let half_n = self.half_n;

        // Extract shifted magnitudes.
        for k in 0..half_n {
            self.shifted_mag[k] = self.shifted_re[k].hypot(self.shifted_im[k]);
        }

        // Compute shifted envelope and correct magnitudes.
        apply_formant_envelope(
            &mut self.shifted_mag,
            &self.envelope_buf,
            &mut self.shifted_envelope_buf,
            half_n,
            shift_ratio,
        );

        // Rescale complex bins to match corrected magnitudes.
        // Guard threshold set well above f32 denormal range to avoid
        // extreme scale factors from near-zero denominators.
        const MAG_FLOOR: f32 = 1e-10;
        for k in 0..half_n {
            let current = self.shifted_re[k].hypot(self.shifted_im[k]);
            if current > MAG_FLOOR {
                let scale = self.shifted_mag[k] / current;
                self.shifted_re[k] *= scale;
                self.shifted_im[k] *= scale;
            }
        }
    }
}

// -- free functions ----------------------------------------------------------

/// Wrap phase into `(-π, π]`.
fn principal_argument(phase: f32) -> f32 {
    phase - TWO_PI * (phase / TWO_PI).round()
}

/// Linear interpolation into `data` at a fractional index.
fn lerp(data: &[f32], index: f32) -> f32 {
    if data.is_empty() || index < 0.0 {
        return 0.0;
    }
    let floor = index as usize;
    if floor + 1 >= data.len() {
        return data[data.len() - 1];
    }
    let frac = index - floor as f32;
    data[floor] * (1.0 - frac) + data[floor + 1] * frac
}

/// Cubic (Catmull-Rom) interpolation into `data` at a fractional index.
///
/// Uses 4 neighbouring samples for a smoother curve than linear.  Falls back
/// to linear at the edges where 4-point support is unavailable.
fn cubic(data: &[f32], index: f32) -> f32 {
    if index < 0.0 {
        return 0.0;
    }
    let n = data.len();
    let i = index as usize;
    if i >= n - 1 {
        return data[n - 1];
    }
    let t = index - i as f32;

    // Need indices i-1, i, i+1, i+2.  Clamp at boundaries.
    let y0 = data[i.saturating_sub(1)];
    let y1 = data[i];
    let y2 = data[(i + 1).min(n - 1)];
    let y3 = data[(i + 2).min(n - 1)];

    // Catmull-Rom spline.
    let a = -0.5 * y0 + 1.5 * y1 - 1.5 * y2 + 0.5 * y3;
    let b = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c = -0.5 * y0 + 0.5 * y2;
    let d = y1;

    ((a * t + b) * t + c) * t + d
}

/// Smoothed spectral envelope (moving average over magnitude).
/// Writes into the provided `out` buffer (must be at least `half_n` long).
fn spectral_envelope_into(magnitude: &[f32], half_n: usize, out: &mut [f32]) {
    let width = (half_n / 32).max(4);
    for (i, env) in out.iter_mut().enumerate().take(half_n) {
        let start = i.saturating_sub(width);
        let end = (i + width).min(half_n);
        let sum: f32 = magnitude[start..end].iter().sum();
        *env = sum / (end - start) as f32;
    }
}

/// Apply formant correction: rescale `shifted_mag` so its spectral envelope
/// matches the original `envelope` (looked up at the pre-shift bin position).
fn apply_formant_envelope(
    shifted_mag: &mut [f32],
    original_envelope: &[f32],
    shifted_envelope: &mut [f32],
    half_n: usize,
    shift_ratio: f32,
) {
    spectral_envelope_into(shifted_mag, half_n, shifted_envelope);
    for k in 0..half_n {
        let env_source = k as f32 * shift_ratio;
        if env_source < (original_envelope.len() - 1) as f32 {
            let target = lerp(original_envelope, env_source);
            let current = shifted_envelope[k];
            if current > 1e-10 {
                shifted_mag[k] *= target / current;
            }
        }
    }
}

/// Linearly blend two packed spectra in the complex domain.
fn mix_complex_spectra(dry: &[f32], wet: &mut [f32], wet_amount: f32, half_n: usize) {
    let dry_amount = 1.0 - wet_amount;

    // DC and Nyquist (real only).
    wet[0] = dry_amount * dry[0] + wet_amount * wet[0];
    wet[1] = dry_amount * dry[1] + wet_amount * wet[1];

    // Interior complex bins.
    let last = half_n - 1;
    for k in 1..last {
        let idx = 2 * k;
        wet[idx] = dry_amount * dry[idx] + wet_amount * wet[idx];
        wet[idx + 1] = dry_amount * dry[idx + 1] + wet_amount * wet[idx + 1];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::assert_within;

    #[test]
    fn principal_argument_wraps_correctly() {
        assert_within!(0.0, principal_argument(0.0), 1e-6);
        // PI and -PI are equivalent; principal_argument may return either.
        assert_within!(PI, principal_argument(PI).abs(), 1e-5);
        assert_within!(PI, principal_argument(3.0 * PI).abs(), 1e-5);
        assert_within!(PI, principal_argument(-3.0 * PI).abs(), 1e-5);
        // Small values pass through unchanged.
        assert_within!(1.0, principal_argument(1.0), 1e-6);
        assert_within!(-1.0, principal_argument(-1.0), 1e-6);
    }

    #[test]
    fn lerp_basic() {
        let data = [0.0f32, 1.0, 2.0, 3.0];
        assert_within!(0.0, lerp(&data, 0.0), 1e-6);
        assert_within!(0.5, lerp(&data, 0.5), 1e-6);
        assert_within!(2.5, lerp(&data, 2.5), 1e-6);
        // Clamp at end.
        assert_within!(3.0, lerp(&data, 10.0), 1e-6);
    }

    #[test]
    fn identity_shift_preserves_spectrum() {
        let window_size = 64;
        let hop_size = 16;
        let mut shifter = SpectralPitchShifter::new(window_size, hop_size);
        shifter.set_shift_ratio(1.0);

        // Single peak at bin 4 — no DC energy to avoid edge-case
        // interactions with region boundaries.
        let mut spectrum = vec![0.0f32; window_size];
        spectrum[8] = 0.5; // bin 4 real
        spectrum[9] = 0.3; // bin 4 imag

        let original = spectrum.clone();
        shifter.transform(&mut spectrum);

        // With ratio=1.0, delta=0 for every peak, and the complex rotation
        // is a multiple of 2π on the first frame → output ≈ input.
        for (i, (&a, &b)) in original.iter().zip(spectrum.iter()).enumerate() {
            assert_within!(a, b, 1e-3, "bin {i}: expected {a}, got {b}");
        }
    }

    #[test]
    fn octave_up_shifts_bins() {
        let window_size = 64;
        let hop_size = 16;
        let mut shifter = SpectralPitchShifter::new(window_size, hop_size);
        shifter.set_shift_semitones(12.0); // octave up → ratio = 2.0

        // Put energy at bin 4 only.
        let mut spectrum = vec![0.0f32; window_size];
        spectrum[8] = 1.0; // bin 4 real

        shifter.transform(&mut spectrum);

        // Bin 8 (= 4 * 2) should now have energy.
        let mag_8 = spectrum[16].hypot(spectrum[17]);
        // Bin 4 should be near zero (shifted away).
        let mag_4 = spectrum[8].hypot(spectrum[9]);
        assert!(
            mag_8 > 0.5,
            "bin 8 should have energy after octave-up: {mag_8}"
        );
        assert!(
            mag_4 < 0.1,
            "bin 4 should be mostly empty after octave-up: {mag_4}"
        );
    }

    #[test]
    fn mix_blends_dry_wet() {
        let window_size = 64;
        let hop_size = 16;
        let mut shifter = SpectralPitchShifter::new(window_size, hop_size);
        shifter.set_shift_semitones(12.0);
        shifter.set_mix(0.0); // fully dry

        let mut spectrum = vec![0.0f32; window_size];
        spectrum[8] = 1.0;
        let original = spectrum.clone();

        shifter.transform(&mut spectrum);

        // mix=0 → output should equal original.
        for (i, (&a, &b)) in original.iter().zip(spectrum.iter()).enumerate() {
            assert_within!(a, b, 1e-6, "mix=0 bin {i}: expected {a}, got {b}");
        }
    }

    #[test]
    fn region_preserves_phase_coherence() {
        // A peak with sidelobes shifted by a fifth: all bins in the region
        // get the same complex rotation, so inter-bin phase relationships
        // from the analysis are preserved in the output.
        let window_size = 128;
        let hop_size = 32;
        let mut shifter = SpectralPitchShifter::new(window_size, hop_size);
        shifter.set_shift_semitones(7.0); // perfect fifth, ratio ≈ 1.498
        shifter.set_mono(true);

        // Simulate a windowed sinusoid: peak at bin 10 with sidelobes.
        let mut spectrum = vec![0.0f32; window_size];
        // Bin 10: magnitude 1.0, phase 0.3
        spectrum[20] = 0.3f32.cos(); // re
        spectrum[21] = 0.3f32.sin(); // im
        // Bin 9: magnitude 0.3, phase 0.5
        spectrum[18] = 0.3 * 0.5f32.cos();
        spectrum[19] = 0.3 * 0.5f32.sin();
        // Bin 11: magnitude 0.3, phase -0.2
        spectrum[22] = 0.3 * (-0.2f32).cos();
        spectrum[23] = 0.3 * (-0.2f32).sin();

        // Input phase differences between sidelobes and peak.
        let input_diff_9_10 = principal_argument(0.5 - 0.3);
        let input_diff_11_10 = principal_argument(-0.2 - 0.3);

        // Run a frame.
        shifter.transform(&mut spectrum);

        // Target bin ≈ round(10 * 1.498) = 15.  Region shifts by +5.
        // Bins 9,10,11 → bins 14,15,16.
        let phase_of = |bin: usize| -> f32 {
            spectrum[2 * bin + 1].atan2(spectrum[2 * bin])
        };

        let p15 = phase_of(15);
        let p14 = phase_of(14);
        let p16 = phase_of(16);

        // The rotation is the same for all bins in the region, so the
        // output inter-bin phase differences should equal the input ones.
        let output_diff_14_15 = principal_argument(p14 - p15);
        let output_diff_16_15 = principal_argument(p16 - p15);

        assert_within!(
            input_diff_9_10, output_diff_14_15, 1e-4,
            "phase diff 14-15 should match input diff 9-10"
        );
        assert_within!(
            input_diff_11_10, output_diff_16_15, 1e-4,
            "phase diff 16-15 should match input diff 11-10"
        );
    }

    #[test]
    fn reset_clears_phase_state() {
        let mut shifter = SpectralPitchShifter::new(64, 16);
        shifter.set_shift_semitones(7.0);

        let mut spectrum = vec![0.0f32; 64];
        spectrum[8] = 1.0;

        // Test poly mode (per-bin accumulator).
        shifter.transform(&mut spectrum);
        assert!(shifter.phase_accumulator.iter().any(|&p| p != 0.0));

        shifter.reset();
        assert!(shifter.phase_accumulator.iter().all(|&p| p == 0.0));
        assert!(shifter.prev_phase.iter().all(|&p| p == 0.0));

        // Test mono mode (synth_phase).
        shifter.set_mono(true);
        spectrum.fill(0.0);
        spectrum[8] = 1.0;
        shifter.transform(&mut spectrum);
        assert!(shifter.synth_phase.iter().any(|&p| p != 0.0));

        shifter.reset();
        assert!(shifter.synth_phase.iter().all(|&p| p == 0.0));
    }

    // ── End-to-end audio tests (T-0260) ────────────────────────────────────

    /// Helper: generate audio, window, FFT, transform, IFFT, overlap-add.
    /// Returns the reconstructed output signal.
    fn pitch_shift_audio(
        signal: &[f32],
        window_size: usize,
        overlap: usize,
        semitones: f32,
        mix: f32,
    ) -> Vec<f32> {
        use crate::fft::RealPackedFft;

        let hop = window_size / overlap;
        let fft = RealPackedFft::new(window_size);
        let mut shifter = SpectralPitchShifter::new(window_size, hop);
        shifter.set_shift_semitones(semitones);
        shifter.set_mix(mix);

        // Hann window
        let hann: Vec<f32> = (0..window_size)
            .map(|i| {
                let n = i as f32 / window_size as f32;
                0.5 * (1.0 - (2.0 * PI * n).cos())
            })
            .collect();

        let out_len = signal.len();
        let mut output = vec![0.0f32; out_len];
        let mut norm = vec![0.0f32; out_len];

        let mut pos = 0isize;
        while (pos as usize) + window_size <= signal.len() + window_size {
            let mut frame = vec![0.0f32; window_size];
            for i in 0..window_size {
                let idx = pos as isize + i as isize;
                if idx >= 0 && (idx as usize) < signal.len() {
                    frame[i] = signal[idx as usize] * hann[i];
                }
            }

            fft.forward(&mut frame);
            shifter.transform(&mut frame);
            fft.inverse(&mut frame);

            // Overlap-add with synthesis window
            for i in 0..window_size {
                let idx = pos as isize + i as isize;
                if idx >= 0 && (idx as usize) < out_len {
                    let oi = idx as usize;
                    output[oi] += frame[i] * hann[i];
                    norm[oi] += hann[i] * hann[i];
                }
            }

            pos += hop as isize;
        }

        // Normalise by WOLA factor
        for i in 0..out_len {
            if norm[i] > 1e-10 {
                output[i] /= norm[i];
            }
        }
        output
    }

    /// 440 Hz sine shifted +12 semitones should produce ~880 Hz.
    #[test]
    fn pitch_shift_octave_up_audio() {
        use crate::fft::RealPackedFft;
        use crate::test_support::dominant_bin;

        let sample_rate = 48_000.0;
        let window_size = 1024;
        let overlap = 4;
        let duration = 8192;

        let signal: Vec<f32> = (0..duration)
            .map(|i| (2.0 * PI * 440.0 / sample_rate * i as f32).sin())
            .collect();

        let output = pitch_shift_audio(&signal, window_size, overlap, 12.0, 1.0);

        // Analyse output with FFT — skip transient at start
        let analysis_start = window_size * 2;
        let fft_size = 2048;
        let fft = RealPackedFft::new(fft_size);
        let mut buf = vec![0.0f32; fft_size];
        for i in 0..fft_size.min(output.len() - analysis_start) {
            buf[i] = output[analysis_start + i];
        }
        fft.forward(&mut buf);

        let peak = dominant_bin(&buf, fft_size);

        let expected_bin = (880.0 * fft_size as f32 / sample_rate).round() as usize;
        let bin_diff = (peak as isize - expected_bin as isize).unsigned_abs();
        assert!(
            bin_diff <= 2,
            "octave up: peak at bin {peak} (expected ~{expected_bin}, 880 Hz)"
        );
    }

    /// Identity shift (0 semitones) should preserve the signal.
    #[test]
    fn pitch_shift_identity_audio() {
        let sample_rate = 48_000.0;
        let window_size = 1024;
        let overlap = 4;
        let duration = 8192;

        let signal: Vec<f32> = (0..duration)
            .map(|i| (2.0 * PI * 440.0 / sample_rate * i as f32).sin())
            .collect();

        let output = pitch_shift_audio(&signal, window_size, overlap, 0.0, 1.0);

        // Compare steady-state region (skip transients)
        let start = window_size * 2;
        let end = duration - window_size;
        let mut sum_sq_signal = 0.0f64;
        let mut sum_sq_error = 0.0f64;
        for i in start..end {
            let s = signal[i] as f64;
            let e = (output[i] - signal[i]) as f64;
            sum_sq_signal += s * s;
            sum_sq_error += e * e;
        }
        let rms_signal = (sum_sq_signal / (end - start) as f64).sqrt();
        let rms_error = (sum_sq_error / (end - start) as f64).sqrt();
        let error_ratio = rms_error / rms_signal;
        assert!(
            error_ratio < 0.1,
            "identity shift error ratio {error_ratio:.4} should be < 0.1"
        );
    }

    /// Mix=0.0 should return the original signal.
    #[test]
    fn pitch_shift_mix_zero_audio() {
        let sample_rate = 48_000.0;
        let window_size = 1024;
        let overlap = 4;
        let duration = 8192;

        let signal: Vec<f32> = (0..duration)
            .map(|i| (2.0 * PI * 440.0 / sample_rate * i as f32).sin())
            .collect();

        let output = pitch_shift_audio(&signal, window_size, overlap, 12.0, 0.0);

        // With mix=0, output should match input in the steady-state region
        let start = window_size * 2;
        let end = duration - window_size;
        let mut sum_sq_signal = 0.0f64;
        let mut sum_sq_error = 0.0f64;
        for i in start..end {
            let s = signal[i] as f64;
            let e = (output[i] - signal[i]) as f64;
            sum_sq_signal += s * s;
            sum_sq_error += e * e;
        }
        let rms_signal = (sum_sq_signal / (end - start) as f64).sqrt();
        let rms_error = (sum_sq_error / (end - start) as f64).sqrt();
        let error_ratio = rms_error / rms_signal;
        assert!(
            error_ratio < 1e-3,
            "mix=0 error ratio {error_ratio:.6} should be < 1e-3"
        );
    }

    #[test]
    fn multiple_peaks_shift_independently() {
        // Two peaks at different frequencies should each shift to their
        // own target bin without interfering.
        let window_size = 128;
        let hop_size = 32;
        let mut shifter = SpectralPitchShifter::new(window_size, hop_size);
        shifter.set_shift_semitones(12.0); // octave up, ratio = 2.0
        shifter.set_mono(true);

        let mut spectrum = vec![0.0f32; window_size];
        // Peak at bin 8.
        spectrum[16] = 1.0;
        // Peak at bin 20.
        spectrum[40] = 0.8;

        shifter.transform(&mut spectrum);

        // Bin 8 → target 16, bin 20 → target 40.
        let mag_16 = spectrum[32].hypot(spectrum[33]);
        let mag_40 = spectrum[2 * 40].hypot(spectrum[2 * 40 + 1]);

        assert!(
            mag_16 > 0.5,
            "target of bin 8 should have energy: {mag_16}"
        );
        assert!(
            mag_40 > 0.4,
            "target of bin 20 should have energy: {mag_40}"
        );

        // Original positions should be mostly empty.
        let mag_8 = spectrum[16].hypot(spectrum[17]);
        let mag_20 = spectrum[40].hypot(spectrum[41]);
        // bin 8 in the output IS the target of the first peak (16→32,
        // but bin 8 = spectrum[16..17] which is now target of bin 8).
        // Actually bin 8 in output = spectrum[16] which is target bin 16's
        // data.  Let me check the original bins:
        // Original bin 8 is at spectrum[16].  Target for peak at bin 8 is
        // bin 16 (spectrum[32]).  So spectrum[16] should be near zero
        // (no peak targets it).  But wait — bin 16 in the output
        // IS the target of peak 8.  Hmm, let me re-examine.
        // Target 16 is at spectrum index 2*16 = 32.  So spectrum[16] is
        // bin 8 in output.  Nothing targets bin 8, so it should be zero.
        assert!(
            mag_8 < 0.1,
            "original bin 8 position should be empty: {mag_8}"
        );
        assert!(
            mag_20 < 0.1,
            "original bin 20 position should be empty: {mag_20}"
        );
    }
}
