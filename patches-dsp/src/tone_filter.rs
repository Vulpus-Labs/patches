//! Per-tap tone filter used by delay modules.
//!
//! A one-pole lowpass whose cutoff is controlled by a `tone` parameter in
//! [0, 1].  At `tone = 1.0` the filter is transparent; at `tone = 0.0` the
//! cutoff is ~200 Hz.  The mapping is logarithmic:
//!
//! ```text
//! cutoff_hz = 200 * (Nyquist / 200) ^ tone
//! alpha     = 1 − exp(−2π · cutoff / sample_rate)
//! ```
//!
//! Because `tone` is not CV-controllable the coefficient is only recomputed
//! via [`set_tone`](ToneFilter::set_tone), which is called from
//! `update_validated_parameters` rather than from `process`.

use std::f32::consts::TAU;

/// One-pole lowpass tone filter with parameter-driven coefficient.
///
/// Call [`prepare`](ToneFilter::prepare) once at module initialisation, then
/// call [`set_tone`](ToneFilter::set_tone) whenever the `tone` parameter
/// changes.  During audio processing call [`process`](ToneFilter::process)
/// with no tone argument.
#[derive(Clone)]
pub struct ToneFilter {
    y_prev:      f32,
    alpha:       f32,
    sample_rate: f32,
}

impl ToneFilter {
    pub fn new() -> Self {
        Self {
            y_prev:      0.0,
            alpha:       1.0, // transparent until prepare() is called
            sample_rate: 44_100.0,
        }
    }

    /// Store the sample rate and recompute the coefficient for the current tone.
    /// Call once at module initialisation.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        // Default tone=1 → passthrough
        self.alpha = 1.0;
    }

    /// Recompute the lowpass coefficient from `tone` ∈ [0, 1].
    ///
    /// At `tone = 1.0` the filter is flat (α = 1.0, y = x).
    /// At `tone = 0.0` the cutoff is ~200 Hz.
    pub fn set_tone(&mut self, tone: f32) {
        let tone = tone.clamp(0.0, 1.0);
        if tone >= 1.0 {
            self.alpha = 1.0;
            return;
        }
        let nyquist = self.sample_rate * 0.5;
        let cutoff = 200.0_f32 * (nyquist / 200.0).powf(tone);
        self.alpha = 1.0 - (-TAU * cutoff / self.sample_rate).exp();
    }

    /// Process one sample through the one-pole lowpass.
    ///
    /// When `alpha == 1.0` (tone=1.0, transparent) the filter is a pure pass-
    /// through and the IIR arithmetic is skipped.  `y_prev` is kept current so
    /// that the filter settles cleanly if `alpha` is later reduced.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        if self.alpha >= 1.0 {
            self.y_prev = x;
            return x;
        }
        let y = self.y_prev + self.alpha * (x - self.y_prev);
        self.y_prev = y;
        y
    }
}

impl Default for ToneFilter {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{assert_reset_deterministic, sine_rms_warmed};

    const SR: f32 = 44_100.0;

    #[test]
    fn tone_one_passes_high_frequencies() {
        let mut f = ToneFilter::new();
        f.prepare(SR);
        f.set_tone(1.0);
        // 10 kHz at tone=1 should pass with < 0.1 dB attenuation relative to 0 dB.
        // RMS of a full sine is 1/√2 ≈ 0.707; at passthrough we expect ≈ 0.707.
        let rms = sine_rms_warmed(10_000.0, SR, 0, 4_410, |x| f.process(x));
        // Allow for up to 0.05 dB attenuation (passthrough tolerance)
        assert!(rms > 0.700, "tone=1 should pass 10 kHz, rms={rms}");
    }

    #[test]
    fn tone_zero_attenuates_high_frequencies() {
        let mut f = ToneFilter::new();
        f.prepare(SR);
        f.set_tone(0.0);
        // Warm up the filter state before measuring
        for _ in 0..4_410 {
            let x = (0.0_f32).sin();
            f.process(x);
        }
        // Reset state for a clean measurement
        f.y_prev = 0.0;
        let rms = sine_rms_warmed(10_000.0, SR, 0, 4_410, |x| f.process(x));
        // At 10 kHz and tone=0 (cutoff ~200 Hz), attenuation should be > 20 dB
        // i.e. rms < 0.707 / 10 ≈ 0.0707
        assert!(rms < 0.08, "tone=0 should attenuate 10 kHz strongly, rms={rms}");
    }

    // ── T2: frequency-response spot checks at 48 kHz ────────────────────────

    const SR_48K: f32 = 48_000.0;

    /// At tone=1.0 the filter must be transparent: all three spot frequencies
    /// should have RMS ≈ 0.707 (within 0.1 dB).
    #[test]
    fn freq_response_tone_one_is_flat() {
        for &freq in &[100.0_f32, 1_000.0, 10_000.0] {
            let mut f = ToneFilter::new();
            f.prepare(SR_48K);
            f.set_tone(1.0);
            let rms = sine_rms_warmed(freq, SR_48K, 4_800, 4_800, |x| f.process(x));
            assert!(
                rms > 0.700,
                "tone=1.0 should be flat at {freq} Hz, rms={rms}"
            );
        }
    }

    /// At tone=0.0 the filter should pass 100 Hz with much more energy than
    /// it passes 10 kHz (dark shelving behaviour).
    #[test]
    fn freq_response_tone_zero_is_dark() {
        let mut f_lo = ToneFilter::new();
        f_lo.prepare(SR_48K);
        f_lo.set_tone(0.0);
        let rms_100 = sine_rms_warmed(100.0, SR_48K, 4_800, 4_800, |x| f_lo.process(x));

        let mut f_hi = ToneFilter::new();
        f_hi.prepare(SR_48K);
        f_hi.set_tone(0.0);
        let rms_10k = sine_rms_warmed(10_000.0, SR_48K, 4_800, 4_800, |x| f_hi.process(x));

        // 100 Hz should be close to passthrough, 10 kHz should be heavily attenuated.
        assert!(rms_100 > 0.60, "tone=0 should pass 100 Hz, rms_100={rms_100}");
        assert!(
            rms_10k < 0.08,
            "tone=0 should attenuate 10 kHz, rms_10k={rms_10k}"
        );
        // Relative: low freq has at least 10× more energy than high freq
        assert!(
            rms_100 > rms_10k * 10.0,
            "tone=0 should be much brighter at 100 Hz than 10 kHz, \
             rms_100={rms_100} rms_10k={rms_10k}"
        );
    }

    /// At tone=1.0 the 1 kHz spot frequency must also pass unattenuated.
    #[test]
    fn freq_response_tone_one_passes_1khz() {
        let mut f = ToneFilter::new();
        f.prepare(SR_48K);
        f.set_tone(1.0);
        let rms = sine_rms_warmed(1_000.0, SR_48K, 4_800, 4_800, |x| f.process(x));
        assert!(rms > 0.700, "tone=1.0 should pass 1 kHz, rms={rms}");
    }

    // ── T7: state reset produces bit-identical output ────────────────────────

    /// Processing the same input sequence twice with a state reset between runs
    /// must yield bit-identical output.
    #[test]
    fn state_reset_produces_identical_output() {
        use std::f32::consts::TAU;
        const FREQ: f32 = 440.0;
        const N: usize = 1_000;
        let inc = FREQ / SR;

        let input: Vec<f32> = (0..N)
            .map(|i| (i as f32 * inc * TAU).sin())
            .collect();

        assert_reset_deterministic!(
            {
                let mut f = ToneFilter::new();
                f.prepare(SR);
                f.set_tone(0.5);
                f
            },
            &input,
            |f: &mut ToneFilter, x: f32| f.process(x),
            |f: &mut ToneFilter| { f.y_prev = 0.0; }
        );
    }

    // ── T-0237: frequency response via impulse-response FFT ─────────────

    use crate::test_support::{
        assert_passband_flat, assert_stopband_below, magnitude_response_db,
    };

    /// Helper: feed a unit impulse through a ToneFilter and collect `len` samples.
    fn impulse_response(tone: f32, sample_rate: f32, len: usize) -> Vec<f32> {
        let mut f = ToneFilter::new();
        f.prepare(sample_rate);
        f.set_tone(tone);
        let mut ir = Vec::with_capacity(len);
        ir.push(f.process(1.0));
        for _ in 1..len {
            ir.push(f.process(0.0));
        }
        ir
    }

    #[test]
    fn tone_one_flat_response_fft() {
        let fft_size = 512;
        let ir = impulse_response(1.0, SR_48K, fft_size);
        let db = magnitude_response_db(&ir, fft_size);

        // Bin range for ~100 Hz to ~20 kHz at 48 kHz / 512
        let bin_100hz = (100.0 * fft_size as f32 / SR_48K).round() as usize; // ~1
        let bin_20khz = (20_000.0 * fft_size as f32 / SR_48K).round() as usize; // ~213
        assert_passband_flat!(db, bin_100hz..=bin_20khz, 0.5);
    }

    #[test]
    fn tone_zero_lowpass_shape_fft() {
        let fft_size = 1024;
        let ir = impulse_response(0.0, SR_48K, fft_size);
        let db = magnitude_response_db(&ir, fft_size);

        // Bins below 100 Hz should be near 0 dB (±3 dB)
        let bin_100hz = (100.0 * fft_size as f32 / SR_48K).round() as usize; // ~2
        // Start from bin 1 (skip DC which can be odd)
        for (bin, &v) in db.iter().enumerate().take(bin_100hz + 1).skip(1) {
            assert!(
                v.abs() <= 3.0,
                "tone=0 low-freq bin {bin} should be near 0 dB, got {v:.2} dB"
            );
        }

        // Bins above 5 kHz should be below -20 dB
        let bin_5khz = (5_000.0 * fft_size as f32 / SR_48K).round() as usize; // ~107
        let bin_nyquist = fft_size / 2;
        assert_stopband_below!(db, bin_5khz..bin_nyquist, -20.0);
    }

    #[test]
    fn tone_half_midpoint_shape_fft() {
        let fft_size = 1024;
        let ir = impulse_response(0.5, SR_48K, fft_size);
        let db = magnitude_response_db(&ir, fft_size);

        // Find the first bin (starting from 1) where response drops below -3 dB
        let minus_3db_bin = (1..fft_size / 2)
            .find(|&b| db[b] < -3.0)
            .expect("tone=0.5 should have a -3 dB point below Nyquist");

        let freq_hz = minus_3db_bin as f32 * SR_48K / fft_size as f32;
        assert!(
            (200.0..=3000.0).contains(&freq_hz),
            "tone=0.5 -3 dB point should be between 200 and 3000 Hz, got {:.1} Hz (bin {})",
            freq_hz,
            minus_3db_bin
        );
    }
}
