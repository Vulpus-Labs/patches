use crate::HalfbandFir;

/// 2× halfband FIR interpolator.
///
/// Given one base-rate input sample it produces two oversampled output samples
/// via zero-insertion followed by the halfband FIR low-pass filter. Both outputs
/// are scaled by 2.0 to compensate for the 0.5× gain introduced by zero-insertion.
///
/// # Group delay
///
/// The same filter kernel as `HalfbandFir` is used, so the group delay is
/// [`GROUP_DELAY_OVERSAMPLED`](HalfbandInterpolator::GROUP_DELAY_OVERSAMPLED) samples
/// at the oversampled rate, or
/// [`GROUP_DELAY_BASE_RATE`](HalfbandInterpolator::GROUP_DELAY_BASE_RATE) at base rate.
pub struct HalfbandInterpolator {
    fir: HalfbandFir,
}

impl HalfbandInterpolator {
    /// Group delay in oversampled samples (same as `HalfbandFir::GROUP_DELAY_OVERSAMPLED`).
    pub const GROUP_DELAY_OVERSAMPLED: usize = HalfbandFir::GROUP_DELAY_OVERSAMPLED;

    /// Group delay in base-rate samples (`GROUP_DELAY_OVERSAMPLED / 2`).
    pub const GROUP_DELAY_BASE_RATE: usize = HalfbandFir::GROUP_DELAY_OVERSAMPLED / 2;

    /// Construct with a custom `HalfbandFir` kernel.
    pub fn new(fir: HalfbandFir) -> Self {
        Self { fir }
    }
}

impl Default for HalfbandInterpolator {
    /// Construct with the default halfband coefficients.
    fn default() -> Self {
        Self {
            fir: HalfbandFir::default(),
        }
    }
}

impl HalfbandInterpolator {

    /// Feed one base-rate sample; returns two oversampled samples `[a, b]`.
    ///
    /// `a` corresponds to the even (real) position, `b` to the odd (zero-inserted)
    /// position. Both are scaled by 2.0 to maintain unity gain at DC.
    #[inline]
    pub fn process(&mut self, x: f32) -> [f32; 2] {
        // Zero-insertion interpolation: push x then 0, evaluate FIR at each position.
        let a = self.fir.push_and_eval(x);
        let b = self.fir.push_and_eval(0.0);
        [a * 2.0, b * 2.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DC input (constant 1.0) must converge to [1.0, 1.0] per pair.
    #[test]
    fn dc_converges() {
        let mut interp = HalfbandInterpolator::default();
        // Settle for enough base-rate samples (filter length / 2 + margin)
        let settle = HalfbandInterpolator::GROUP_DELAY_BASE_RATE + 32;
        let mut last = [0.0_f32; 2];
        for _ in 0..(settle + 1) {
            last = interp.process(1.0);
        }
        assert!(
            (last[0] - 1.0).abs() < 0.01,
            "DC a channel: expected ~1.0, got {}",
            last[0]
        );
        assert!(
            (last[1] - 1.0).abs() < 0.01,
            "DC b channel: expected ~1.0, got {}",
            last[1]
        );
    }

    /// Base-rate Nyquist (+1/-1 at base rate) maps to oversampled_rate/4 after
    /// interpolation.  For the default halfband FIR the passband extends to
    /// oversampled_rate/4, so this tone passes at close to unity gain; both
    /// channels should settle to a non-trivial amplitude (≥ 0.5) after the
    /// filter has settled, confirming the interpolator is not suppressing it.
    #[test]
    fn base_rate_nyquist_passes_through() {
        let mut interp = HalfbandInterpolator::default();
        let settle = HalfbandInterpolator::GROUP_DELAY_BASE_RATE + 64;
        let mut sum_sq = 0.0_f32;
        let mut count = 0_usize;
        for i in 0..(settle + 64 + 1) {
            let x = if i % 2 == 0 { 1.0_f32 } else { -1.0_f32 };
            let [a, _b] = interp.process(x);
            if i >= settle {
                sum_sq += a * a;
                count += 1;
            }
        }
        let rms = (sum_sq / count as f32).sqrt();
        assert!(
            rms > 0.5,
            "Base-rate Nyquist should pass through the interpolator (rms={rms})"
        );
    }

    /// A 1 kHz sine at 48 kHz base rate must produce oversampled output within 0.1 dB.
    #[test]
    fn passband_1khz_within_0_1_db() {
        let base_rate = 48_000.0_f32;
        let freq = 1_000.0_f32;
        let n = 4800_usize;
        let settle = HalfbandInterpolator::GROUP_DELAY_BASE_RATE + 64;
        let total = n + settle;
        let two_pi = 2.0 * std::f32::consts::PI;

        let mut interp = HalfbandInterpolator::default();
        let mut sum_sq_in = 0.0_f32;
        let mut sum_sq_out_a = 0.0_f32;
        let mut count = 0_usize;

        for i in 0..total {
            let x = (two_pi * freq * i as f32 / base_rate).sin();
            let [a, _b] = interp.process(x);
            if i >= settle {
                sum_sq_in += x * x;
                sum_sq_out_a += a * a;
                count += 1;
            }
        }

        let rms_in = (sum_sq_in / count as f32).sqrt();
        let rms_out = (sum_sq_out_a / count as f32).sqrt();
        let db = 20.0 * (rms_out / rms_in).log10();
        assert!(
            db.abs() < 0.1,
            "1 kHz passband: {db:.4} dB amplitude error exceeds 0.1 dB limit"
        );
    }

    /// After 2× interpolation, a 10 kHz input creates an image at `fs_base − 10 kHz = 38 kHz`
    /// in the oversampled (96 kHz) domain.  38 kHz is well into the halfband stopband
    /// (stopband starts above 24 kHz = fs_over / 4).  The filter should attenuate the image
    /// by at least 60 dB relative to the signal level at 10 kHz.
    ///
    /// We measure using a DFT bin correlation to isolate each frequency in the 96 kHz
    /// output stream.
    #[test]
    fn stopband_image_is_attenuated_by_at_least_60_db() {
        let base_rate = 48_000.0_f32;
        let over_rate = 2.0 * base_rate;   // 96 000 Hz
        let freq_signal = 10_000.0_f32;    // desired tone (well inside passband)
        let freq_image  = base_rate - freq_signal; // 38 000 Hz (well inside stopband)

        let n_input = 9600_usize; // 200 ms at 48 kHz — long window for sharp frequency resolution
        let settle = HalfbandInterpolator::GROUP_DELAY_BASE_RATE + 64;
        let total_input = n_input + settle;
        let two_pi = 2.0 * std::f32::consts::PI;

        let mut interp = HalfbandInterpolator::default();

        // DFT bin correlation: accumulate phasors at signal and image frequencies.
        let mut sig_re = 0.0_f32;
        let mut sig_im = 0.0_f32;
        let mut img_re = 0.0_f32;
        let mut img_im = 0.0_f32;
        let sig_dphi = two_pi * freq_signal / over_rate;
        let img_dphi = two_pi * freq_image  / over_rate;
        let mut sig_phi = 0.0_f32;
        let mut img_phi = 0.0_f32;

        for i in 0..total_input {
            let x = (two_pi * freq_signal * i as f32 / base_rate).sin();
            let [a, b] = interp.process(x);
            // Each input sample produces two output samples (a then b)
            for &y in &[a, b] {
                if i >= settle {
                    sig_re += y * sig_phi.cos();
                    sig_im += y * sig_phi.sin();
                    img_re += y * img_phi.cos();
                    img_im += y * img_phi.sin();
                }
                sig_phi += sig_dphi;
                img_phi += img_dphi;
            }
        }

        let amp_signal = (sig_re * sig_re + sig_im * sig_im).sqrt();
        let amp_image  = (img_re * img_re + img_im * img_im).sqrt();

        assert!(amp_signal > 0.0, "Signal bin has zero power — check test setup");
        let db = 20.0 * (amp_image / amp_signal).log10();
        assert!(
            db <= -60.0,
            "38 kHz image (stopband): {db:.2} dB relative to 10 kHz signal — expected ≤ -60 dB"
        );
    }

    #[test]
    fn group_delay_base_rate_is_half_of_oversampled() {
        assert_eq!(
            HalfbandInterpolator::GROUP_DELAY_BASE_RATE,
            HalfbandFir::GROUP_DELAY_OVERSAMPLED / 2
        );
    }

    // ── T7 — determinism and state reset ─────────────────────────────────────

    /// T7 — determinism and state reset
    /// Two fresh HalfbandInterpolator instances fed the same input sequence
    /// must produce bit-identical output (checked per channel).
    #[test]
    fn halfband_interpolator_determinism() {
        use crate::test_support::assert_deterministic;

        let n = 200_usize;
        let two_pi = std::f32::consts::TAU;
        let input: Vec<f32> = (0..n)
            .map(|i| (two_pi * 440.0 * i as f32 / 48_000.0).sin())
            .collect();

        // Check channel 0.
        assert_deterministic!(
            HalfbandInterpolator::default(),
            &input,
            |interp: &mut HalfbandInterpolator, x: f32| interp.process(x)[0]
        );

        // Check channel 1.
        assert_deterministic!(
            HalfbandInterpolator::default(),
            &input,
            |interp: &mut HalfbandInterpolator, x: f32| interp.process(x)[1]
        );
    }

    // ── T4 — stability and convergence ───────────────────────────────────────

    /// T4 — stability and convergence
    /// Feed HalfbandInterpolator with 10,000 samples alternating between 1.0
    /// and -1.0 (maximum amplitude). Every output [a, b] must be finite and
    /// |a| < 3.0 and |b| < 3.0.
    #[test]
    fn halfband_interpolator_stability_at_max_amplitude() {
        let mut interp = HalfbandInterpolator::default();
        for i in 0..10_000_usize {
            let x = if i % 2 == 0 { 1.0_f32 } else { -1.0_f32 };
            let [a, b] = interp.process(x);
            assert!(a.is_finite(), "output[0] not finite at sample {i}: {a}");
            assert!(b.is_finite(), "output[1] not finite at sample {i}: {b}");
            assert!(
                a.abs() < 3.0,
                "output[0] out of bounds at sample {i}: {a}"
            );
            assert!(
                b.abs() < 3.0,
                "output[1] out of bounds at sample {i}: {b}"
            );
        }
    }
}
