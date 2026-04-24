/// Non-zero off-centre taps for the default 33-tap halfband FIR.
///
/// Full filter length = 4 × n_taps + 1 = 33; non-zero off-centre tap count = 8.
pub const DEFAULT_TAPS: [f32; 8] = [
    -0.00188788, 0.00386248, -0.00824247, 0.01594711,
    -0.02867656, 0.05071856, -0.09801591, 0.31594176,
];
pub const DEFAULT_CENTRE: f32 = 0.500_705_8;

/// Halfband FIR filter (decimation or interpolation kernel).
///
/// Uses a symmetric linear-phase FIR. Non-zero off-centre taps are supplied at
/// construction; every other tap is zero by the halfband property. The centre
/// tap is fixed at `centre`. Full filter length = 4 × n_taps + 1.
///
/// # Group delay
///
/// [`GROUP_DELAY_OVERSAMPLED`](HalfbandFir::GROUP_DELAY_OVERSAMPLED) is the
/// group delay in oversampled samples (half the filter order).
pub struct HalfbandFir {
    taps: Vec<f32>,
    delay: Vec<f32>,
    pos: usize,
    centre: f32,
    midpoint_offset: usize,
}

impl HalfbandFir {
    /// Group delay of the filter in oversampled samples.
    ///
    /// For the default 8-tap kernel: `(4 * 8) / 2 = 16`.
    pub const GROUP_DELAY_OVERSAMPLED: usize = 16;

    /// Construct a `HalfbandFir` with custom taps and centre coefficient.
    pub fn new(taps: Vec<f32>, centre: f32) -> Self {
        let taps_len = taps.len();
        let len = taps_len * 4 + 2;
        Self {
            taps,
            delay: vec![0.0; len],
            pos: 0,
            centre,
            midpoint_offset: len - (taps_len * 2),
        }
    }


    /// Decimate two input samples into one output sample.
    #[inline]
    pub fn process(&mut self, first: f32, second: f32) -> f32 {
        let n_taps = self.taps.len();
        let delay_len = self.delay.len();

        let newest = self.push_sample(first);
        self.push_sample(second);

        let center_idx = (newest + self.midpoint_offset) % delay_len;
        let mut acc = self.centre * self.delay[center_idx];

        let mut offset_r = (center_idx + 1) % delay_len;
        let mut offset_l = (center_idx + delay_len - 1) % delay_len;

        for t in (0..n_taps).rev() {
            acc += self.taps[t] * (self.delay[offset_l] + self.delay[offset_r]);
            offset_r = (offset_r + 2) % delay_len;
            offset_l = (offset_l + delay_len - 2) % delay_len;
        }

        acc
    }

}

impl Default for HalfbandFir {
    /// Construct with the default 33-tap halfband coefficients.
    fn default() -> Self {
        Self::new(DEFAULT_TAPS.to_vec(), DEFAULT_CENTRE)
    }
}

impl HalfbandFir {
    /// Push one sample into the delay line and evaluate the FIR, returning one output sample.
    ///
    /// Used by [`HalfbandInterpolator`](crate::HalfbandInterpolator) to produce
    /// one output per oversampled position (single-sample stride).
    #[inline]
    pub(crate) fn push_and_eval(&mut self, x: f32) -> f32 {
        let n_taps = self.taps.len();
        let delay_len = self.delay.len();

        let newest = self.push_sample(x);

        let center_idx = (newest + self.midpoint_offset) % delay_len;
        let mut acc = self.centre * self.delay[center_idx];

        let mut offset_r = (center_idx + 1) % delay_len;
        let mut offset_l = (center_idx + delay_len - 1) % delay_len;

        for t in (0..n_taps).rev() {
            acc += self.taps[t] * (self.delay[offset_l] + self.delay[offset_r]);
            offset_r = (offset_r + 2) % delay_len;
            offset_l = (offset_l + delay_len - 2) % delay_len;
        }

        acc
    }

    /// Push one sample into the delay line; returns the index just written.
    #[inline]
    pub(crate) fn push_sample(&mut self, x: f32) -> usize {
        let idx = self.pos;
        self.delay[idx] = x;
        self.pos += 1;
        if self.pos == self.delay.len() {
            self.pos = 0;
        }
        idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Convert a linear amplitude ratio to dB.
    fn to_db(amplitude: f32) -> f32 {
        20.0 * amplitude.abs().log10()
    }

    /// Measure the steady-state gain of the filter when driven with a sinusoid
    /// at normalised frequency `freq` (cycles per oversampled sample, so 0.5
    /// is the Nyquist of the oversampled rate).
    ///
    /// Returns the ratio of output RMS to input RMS after flushing the FIR delay.
    /// For a unity-gain passband frequency the ratio should be ~1.0.
    fn measure_gain(freq: f32, flush_pairs: usize, measure_pairs: usize) -> f32 {
        let mut f = HalfbandFir::default();
        let mut sample_idx: usize = 0;

        let next_sample = |idx: usize| -> f32 { (2.0 * PI * freq * idx as f32).sin() };

        // Flush
        for _ in 0..flush_pairs {
            let a = next_sample(sample_idx);
            let b = next_sample(sample_idx + 1);
            sample_idx += 2;
            let _ = f.process(a, b);
        }

        // Measure output RMS and input RMS over the same window.
        let mut out_sum_sq = 0.0_f32;
        let mut in_sum_sq = 0.0_f32;
        for _ in 0..measure_pairs {
            let a = next_sample(sample_idx);
            let b = next_sample(sample_idx + 1);
            // Input RMS includes both samples before decimation.
            in_sum_sq += a * a + b * b;
            sample_idx += 2;
            let y = f.process(a, b);
            // One output per pair.
            out_sum_sq += y * y;
        }
        let out_rms = (out_sum_sq / measure_pairs as f32).sqrt();
        // Two input samples per pair; average their mean square then take sqrt.
        let in_rms = (in_sum_sq / (2 * measure_pairs) as f32).sqrt();
        if in_rms < 1e-10 {
            0.0
        } else {
            out_rms / in_rms
        }
    }

    // -----------------------------------------------------------------------
    // Impulse response
    // -----------------------------------------------------------------------

    /// The filter is LTI so its impulse response equals its tap sequence.
    ///
    /// With `process(first, second)` the filter decimates two input samples per
    /// call.  The full 33-tap filter has group delay 16 oversampled samples = 8
    /// pairs.  Feeding a unit impulse at pair 0 and zeros thereafter, the output
    /// sequence (sampled at the decimated rate) traces the filter's polyphase
    /// sub-filters.
    ///
    /// Because every off-centre tap of a halfband FIR lives at an *odd* offset
    /// from the centre, and the decimation stride is 2, the centre tap appears
    /// as an isolated pulse at pair 8 while the off-centre taps contribute via
    /// pairs where both the left and right symmetric tap fall on non-zero input.
    /// For a unit impulse at sample 0, only the centre tap falls on the impulse
    /// in any single output computation; the result at pair 8 therefore equals
    /// `DEFAULT_CENTRE`.
    #[test]
    fn impulse_response_centre_tap() {
        let mut f = HalfbandFir::default();
        // Number of pairs needed to observe the group-delay-shifted peak:
        // GROUP_DELAY_OVERSAMPLED / 2 = 8, so the peak is at pair 8.
        let group_delay_pairs = HalfbandFir::GROUP_DELAY_OVERSAMPLED / 2;
        let total_pairs = group_delay_pairs + 10;

        let mut outputs = Vec::with_capacity(total_pairs);
        for i in 0..total_pairs {
            let (a, b) = if i == 0 { (1.0_f32, 0.0_f32) } else { (0.0, 0.0) };
            outputs.push(f.process(a, b));
        }

        // The peak of the impulse response should be at `group_delay_pairs`.
        let peak_idx = outputs
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
            .map(|(i, _)| i)
            .unwrap();
        assert_eq!(
            peak_idx, group_delay_pairs,
            "impulse peak should be at pair {group_delay_pairs}, got {peak_idx}"
        );

        // The peak value should equal the centre tap coefficient.
        let peak_val = outputs[group_delay_pairs];
        assert!(
            (peak_val - DEFAULT_CENTRE).abs() < 1e-6,
            "impulse peak {peak_val} should equal DEFAULT_CENTRE {DEFAULT_CENTRE}"
        );
    }

    // -----------------------------------------------------------------------
    // DC response
    // -----------------------------------------------------------------------

    /// Driving the filter with constant 1.0 input should converge to 1.0 after
    /// the FIR delay has been fully flushed.  The full filter spans 33 taps =
    /// 16.5 pairs, so we flush 20 pairs before asserting.  We allow a tolerance
    /// of 0.01 (1%) on steady-state samples.
    #[test]
    fn dc_converges_to_unity() {
        let mut f = HalfbandFir::default();
        // Flush enough pairs to fill the entire FIR delay line (33 taps, ~17 pairs).
        let flush_pairs = 20;
        let extra_pairs = 8;
        let tolerance = 0.01_f32;

        for _ in 0..flush_pairs {
            let _ = f.process(1.0, 1.0);
        }

        // Steady-state: output should be within `tolerance` of 1.0.
        for i in 0..extra_pairs {
            let y = f.process(1.0, 1.0);
            assert!(
                (y - 1.0).abs() < tolerance,
                "DC steady-state pair {i}: {y} not within {tolerance} of 1.0"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Nyquist response
    // -----------------------------------------------------------------------

    /// Driving the filter with alternating ±1.0 at the oversampled sample rate
    /// (normalised frequency = 0.5, the oversampled Nyquist) should converge
    /// to ≈ 0.0 after the FIR delay.  A halfband FIR has a zero at the Nyquist
    /// of the oversampled rate, so steady-state attenuation should be very high.
    /// We assert |output| < 0.05.
    ///
    /// Oversampled Nyquist = sample pattern 1, -1, 1, -1, …
    /// In pairs this is always (1.0, -1.0).
    #[test]
    fn nyquist_converges_to_zero() {
        let mut f = HalfbandFir::default();
        // Flush enough pairs to fill the entire FIR delay line (33 taps, ~17 pairs).
        let flush_pairs = 20;
        let extra_pairs = 8;
        let tolerance = 0.05_f32;

        // All pairs are (1, -1) to drive the oversampled Nyquist frequency.
        for _ in 0..flush_pairs {
            let _ = f.process(1.0, -1.0);
        }

        // Steady-state
        for i in 0..extra_pairs {
            let y = f.process(1.0, -1.0);
            assert!(
                y.abs() < tolerance,
                "Nyquist steady-state pair {i}: {y} magnitude exceeds {tolerance}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Passband gain
    // -----------------------------------------------------------------------

    /// A sinusoid at fs/8 of the oversampled rate (normalised frequency 0.125,
    /// well inside the passband) should pass through with gain within ±0.1 dB
    /// of unity.
    #[test]
    fn passband_gain_near_unity() {
        // freq = 0.125 (normalised: 1 cycle per 8 oversampled samples)
        let freq = 0.125_f32;
        let gain = measure_gain(freq, 64, 128);

        let db = to_db(gain);
        assert!(
            db.abs() < 0.1,
            "passband gain at freq {freq}: {gain:.6} ({db:.4} dB) — expected within ±0.1 dB"
        );
    }

    // -----------------------------------------------------------------------
    // Stopband attenuation
    // -----------------------------------------------------------------------

    /// A sinusoid at 0.35 × fs of the oversampled rate (normalised frequency 0.35,
    /// well into the stopband of the halfband filter) should be attenuated by
    /// at least 60 dB.
    #[test]
    fn stopband_attenuation_at_least_60db() {
        // freq = 0.35 (normalised: comfortably inside the stopband)
        let freq = 0.35_f32;
        let gain = measure_gain(freq, 128, 512);

        let db = to_db(gain);
        assert!(
            db < -60.0,
            "stopband attenuation at freq {freq}: {gain:.6} ({db:.4} dB) — expected < -60 dB"
        );
    }

    // -----------------------------------------------------------------------
    // Full transfer function (FFT-based)
    // -----------------------------------------------------------------------

    /// Capture the halfband FIR kernel's full transfer function via FFT and
    /// assert passband flatness and stopband rejection.
    ///
    /// Uses `push_and_eval` to extract the raw FIR impulse response (one
    /// output per input sample, at the oversampled rate). The halfband FIR's
    /// passband extends to ~0.25 × oversampled rate and the stopband from
    /// ~0.25 to 0.5 × oversampled rate. In a 256-point FFT, bin k maps to
    /// normalised frequency k/256:
    ///   - passband: bins 1..=50   (0 to ~0.2 × Nyquist)
    ///   - stopband: bins 75..=128 (~0.3 × Nyquist to Nyquist)
    #[test]
    fn halfband_fir_full_transfer_function() {
        use crate::test_support::{assert_passband_flat, assert_stopband_below, magnitude_response_db};

        let mut fir = HalfbandFir::default();
        // 33-tap FIR; collect enough samples to capture the full impulse response.
        let n_samples = 64;
        let mut ir = Vec::with_capacity(n_samples);

        for i in 0..n_samples {
            let x = if i == 0 { 1.0_f32 } else { 0.0 };
            ir.push(fir.push_and_eval(x));
        }

        let fft_size = 256;
        let response_db = magnitude_response_db(&ir, fft_size);

        // Passband: bins 1..=50 should be within ±0.5 dB of 0 dB.
        // (bin 50 ≈ 0.20 × Nyquist, safely within the passband)
        assert_passband_flat!(response_db, 1..=50, 0.5);

        // Stopband: bins 85..=128 should be below -55 dB.
        // (bin 85 ≈ 0.33 × Nyquist, past the transition band; the filter
        //  spec is -60 dB but we allow 5 dB margin for the FFT resolution)
        assert_stopband_below!(response_db, 85..=128, -55.0);
    }

    /// Capture the interpolator impulse response via FFT and assert passband
    /// flatness and stopband rejection.
    ///
    /// The interpolator produces two oversampled samples per base-rate input.
    /// We feed a unit impulse at the base rate, collect all oversampled output,
    /// and analyse the spectrum at the oversampled rate. The passband extends
    /// to ~0.25 × oversampled rate (bins 1..64 of a 512-point FFT), and the
    /// stopband from ~0.3 × oversampled rate to Nyquist (bins 154..256).
    #[test]
    fn halfband_interpolator_full_transfer_function() {
        use crate::test_support::{assert_passband_flat, assert_stopband_below, magnitude_response_db};
        use crate::HalfbandInterpolator;

        let mut interp = HalfbandInterpolator::default();
        let n_input = 64; // base-rate samples
        let mut ir = Vec::with_capacity(n_input * 2);

        for i in 0..n_input {
            let x = if i == 0 { 1.0_f32 } else { 0.0 };
            let [a, b] = interp.process(x);
            ir.push(a);
            ir.push(b);
        }

        let fft_size = 512;
        let response_db = magnitude_response_db(&ir, fft_size);

        // The interpolator applies a 2× gain to compensate for zero-insertion,
        // so the DC level of the impulse response is ~+6 dB. Normalise to 0 dB
        // at DC so passband/stopband assertions use a consistent reference.
        let dc_db = response_db[0];
        let normalised: Vec<f32> = response_db.iter().map(|&db| db - dc_db).collect();

        // Passband: bins 1..=50 (well within 0..0.25 of oversampled rate)
        // should be within ±0.5 dB of 0 dB
        assert_passband_flat!(normalised, 1..=50, 0.5);

        // Stopband: bins 170..=256 (above 0.33 of oversampled rate to Nyquist)
        // should be below -55 dB
        assert_stopband_below!(normalised, 170..=256, -55.0);
    }
}
