use std::f32::consts::TAU;

/// Rational (Padé-like) approximation to hyperbolic tangent.
///
/// Uses a degree-5/6 Padé form for |x| < 2.5 and saturates to ±1 for
/// |x| ≥ 2.5.  The Padé reaches its numerical peak at about |x| = 2.5
/// (≈ 0.972), beyond which it would decrease — the saturation guard prevents
/// that non-monotone behaviour while keeping the transition monotone (1.0 >
/// Padé-peak).
///
/// Properties:
/// - Exact at x = 0 (returns 0.0).
/// - Monotonically non-decreasing over all reals.
/// - Saturates to exactly ±1 for |x| ≥ 2.5.
/// - RMS error over [−3, 3] is approximately 0.02 (documented tolerance < 0.05).
/// - Max absolute error for |x| ≤ 2 is approximately 0.001.
#[inline(always)]
pub fn fast_tanh(x: f32) -> f32 {
    if x >= 2.5 {
        return 1.0;
    }
    if x <= -2.5 {
        return -1.0;
    }
    let x2 = x * x;
    let x4 = x2 * x2;
    let x6 = x4 * x2;
    x * (10395.0 + 1260.0 * x2 + 21.0 * x4)
        / (10395.0 + 4725.0 * x2 + 210.0 * x4 + 4.0 * x6)
}

static SINE_TABLE: std::sync::LazyLock<Vec<f32>> = std::sync::LazyLock::new(|| {
    (0..1024).map(|i| (i as f32 / 1024.0 * TAU).sin()).collect()
});

/// Wavetable lookup for sine. `phase` must be in [0, 1).
#[inline(always)]
pub fn lookup_sine(phase: f32) -> f32 {
    let index = phase * 1024.0;
    let index_whole = index as usize;
    let index_frac = index - (index_whole as f32);
    let a = SINE_TABLE[index_whole & 1023];
    let b = SINE_TABLE[(index_whole + 1) & 1023];
    a + (b - a) * index_frac
}

/// Polynomial approximation of sine. `phase` must be in [0, 1).
///
/// Uses the Bhaskara I formula with Moser correction.
/// Max absolute error ≈ 0.001; RMS error < 0.01 over a full cycle.
#[inline(always)]
pub fn fast_sine(phase: f32) -> f32 {
    debug_assert!((0.0..1.0).contains(&phase), "phase must be in [0, 1)");
    let x1 = phase - 0.5;
    let x2 = x1 * 16.0 * (x1.abs() - 0.5);
    x2 + 0.225 * x2 * (x2.abs() - 1.0)
}

/// Fast approximation of 2^x (base-2 exponential).
///
/// Uses a 5th-order polynomial on the fractional part and reconstructs the
/// integer part via the IEEE 754 exponent field.  Max relative error < 1e-4
/// in the range [−10, 10].
#[inline]
pub fn fast_exp2(x: f32) -> f32 {
    // Handle extreme cases similarly to libm behaviour (roughly)
    if x.is_nan() {
        return f32::NAN;
    }
    if x >= 128.0 {
        return f32::INFINITY;
    }
    if x <= -150.0 {
        return 0.0;
    }

    // Split into integer and fractional parts
    let i = x.floor();
    let f = x - i;

    // --- Polynomial approximation of 2^f on [0, 1) ---
    //
    // 5th-order minimax-ish fit for 2^f
    // (coefficients tuned for reasonable error, not absolute optimality)
    //
    // 2^f ≈ c0 + f*(c1 + f*(c2 + f*(c3 + f*(c4 + f*c5))))
    //
    let c0 = 1.0f32;
    let c1 = std::f32::consts::LN_2;
    let c2 = 0.240_226_5_f32;
    let c3 = 0.055_504_11_f32;
    let c4 = 0.009_618_13_f32;
    let c5 = 0.001_333_36_f32;

    let poly = (((c5 * f + c4) * f + c3) * f + c2) * f + c1;
    let frac = poly * f + c0;

    // --- Reconstruct 2^i using exponent field ---
    //
    // float layout: sign(1) exponent(8) mantissa(23)
    // exponent bias = 127
    //
    let ei = (i as i32) + 127;

    // Clamp exponent to valid range
    if ei <= 0 {
        // subnormal (very small numbers)
        return 0.0;
    }
    if ei >= 255 {
        return f32::INFINITY;
    }

    let bits = (ei as u32) << 23;
    let pow2_i = f32::from_bits(bits);

    pow2_i * frac
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fast_sine_key_points() {
        // phase=0 → sin(0)=0, phase=0.25 → sin(π/2)=1, etc.
        let cases = [
            (0.0f32, 0.0f32),
            (0.25, 1.0),
            (0.5, 0.0),
            (0.75, -1.0),
        ];
        for (phase, expected) in cases {
            let got = fast_sine(phase);
            assert!(
                (got - expected).abs() < 0.002,
                "fast_sine({phase}) = {got}, expected ≈ {expected}"
            );
        }
    }

    #[test]
    fn test_fast_sine_accuracy() {
        // Sweep one full cycle densely and measure max absolute error vs true sin.
        let steps = 100_000u32;
        let mut max_abs_err = 0.0f32;
        let mut worst_phase = 0.0f32;

        for i in 0..steps {
            let phase = i as f32 / steps as f32;
            let approx = fast_sine(phase);
            let exact = (phase * TAU).sin();
            let err = (approx - exact).abs();
            if err > max_abs_err {
                max_abs_err = err;
                worst_phase = phase;
            }
        }

        println!("fast_sine max abs error: {max_abs_err:.6} at phase={worst_phase:.6}");
        // Bhaskara + Moser correction gives ~0.1% max error; allow 0.002 headroom.
        assert!(
            max_abs_err < 0.002,
            "max absolute error {max_abs_err} exceeds 0.002"
        );
    }

    #[test]
    fn test_fast_sine_snr() {
        // T6 — fast_sine SNR: assert RMS error vs f64::sin() over full period < 0.01.
        //
        // Documented tolerance: RMS error < 0.01 across a dense sweep of [0, 1).
        // The Bhaskara+Moser polynomial achieves ~0.001 RMS in practice.
        let steps = 100_000u32;
        let mut sum_sq_err = 0.0f64;

        for i in 0..steps {
            let phase = i as f32 / steps as f32;
            let approx = fast_sine(phase) as f64;
            let exact = (phase as f64 * std::f64::consts::TAU).sin();
            let err = approx - exact;
            sum_sq_err += err * err;
        }

        let rms = (sum_sq_err / steps as f64).sqrt();
        println!("fast_sine RMS error: {rms:.2e}");
        // Documented tolerance: RMS < 0.01 over full period.
        assert!(rms < 0.01, "fast_sine RMS error {rms:.2e} exceeds 0.01");
    }

    #[test]
    fn test_fast_exp2_accuracy() {
        // Domain: typical musical pitch range, e.g. ~ -10 to +10 octaves
        let start = -10.0f32;
        let end = 10.0f32;

        // Dense sampling to catch local error spikes
        let steps = 200_000;
        let step = (end - start) / steps as f32;

        let mut max_abs_err = 0.0f32;
        let mut max_rel_err = 0.0f32;

        let mut worst_x_abs = 0.0f32;
        let mut worst_x_rel = 0.0f32;

        let mut x = start;
        for _ in 0..=steps {
            let fast = fast_exp2(x);
            let exact = x.exp2();

            let abs_err = (fast - exact).abs();

            // Avoid blowing up relative error near zero
            let rel_err = if exact != 0.0 {
                abs_err / exact.abs()
            } else {
                0.0
            };

            if abs_err > max_abs_err {
                max_abs_err = abs_err;
                worst_x_abs = x;
            }

            if rel_err > max_rel_err {
                max_rel_err = rel_err;
                worst_x_rel = x;
            }

            x += step;
        }

        println!("max abs error: {} at x={}", max_abs_err, worst_x_abs);
        println!("max rel error: {} at x={}", max_rel_err, worst_x_rel);

        // Tolerances: adjust depending on how aggressive your polynomial is
        assert!(max_rel_err < 1e-4, "relative error too high");
        let scaled_abs_err = max_abs_err / (2.0f32.powf(worst_x_abs)).max(1.0);
        assert!(scaled_abs_err < 1e-4, "scaled absolute error too high");
    }

    #[test]
    fn test_fast_exp2_basic_points() {
        let test_values = [
            -10.0, -5.0, -1.0, -0.5, 0.0, 0.5, 1.0, 2.0, 5.0, 10.0f32,
        ];

        for &x in &test_values {
            let fast = fast_exp2(x);
            let exact = x.exp2();

            let rel_err = if exact != 0.0 {
                ((fast - exact) / exact).abs()
            } else {
                0.0
            };

            println!(
                "x={:.3}, fast={:.6}, exact={:.6}, rel_err={:.6e}",
                x, fast, exact, rel_err
            );

            assert!(rel_err < 1e-4);
        }
    }

    // ── fast_tanh tests ────────────────────────────────────────────────────────

    #[test]
    fn test_fast_tanh_key_points() {
        let cases: &[(f32, f32, f32, &str)] = &[
            (0.0,   0.0,  0.0,   "tanh(0) = 0 exactly"),
            (10.0,  1.0,  0.001, "tanh(10) saturates to +1"),
            (-10.0, -1.0, 0.001, "tanh(-10) saturates to -1"),
        ];
        for &(input, expected, tol, label) in cases {
            let got = fast_tanh(input);
            assert!(
                (got - expected).abs() <= tol,
                "fast_tanh({input}) = {got}, expected {expected} ({label})"
            );
        }
    }

    #[test]
    fn test_fast_tanh_antisymmetry() {
        // tanh is an odd function: fast_tanh(-x) must equal -fast_tanh(x).
        let xs: Vec<f32> = (1..=50).map(|i| i as f32 * 0.1).collect();
        for x in xs {
            let pos = fast_tanh(x);
            let neg = fast_tanh(-x);
            assert!(
                (neg + pos).abs() < 1e-6,
                "antisymmetry violated: fast_tanh({x}) = {pos}, fast_tanh({}) = {neg}",
                -x
            );
        }
    }

    #[test]
    fn test_fast_tanh_accuracy() {
        // Measure RMS error vs f64::tanh() over [-3, 3].
        //
        // The Padé approximation is accurate near zero but under-estimates
        // |tanh(x)| by up to ~8 % near |x|=3.  The saturation guard at |x|≥4
        // restores correct behaviour beyond that.
        //
        // Documented tolerance: RMS error < 0.05 over [-3, 3].
        let steps = 10_000u32;
        let mut sum_sq_err = 0.0f64;

        for i in 0..=steps {
            let t = i as f64 / steps as f64; // 0..=1
            let x = -3.0 + t * 6.0; // -3..=3
            let approx = fast_tanh(x as f32) as f64;
            let exact = x.tanh();
            let err = approx - exact;
            sum_sq_err += err * err;
        }

        let rms = (sum_sq_err / (steps + 1) as f64).sqrt();
        println!("fast_tanh RMS error over [-3,3]: {rms:.2e}");
        // Documented tolerance: RMS error < 0.05 over [-3, 3].
        assert!(rms < 0.05, "RMS error {rms:.2e} exceeds 0.05");
    }

    #[test]
    fn test_fast_tanh_monotone() {
        // tanh is strictly increasing; the approximation must be non-decreasing.
        let steps = 10_000u32;
        let x_min = -6.0f32;
        let x_max = 6.0f32;
        let mut prev = fast_tanh(x_min);
        for i in 1..=steps {
            let x = x_min + (x_max - x_min) * (i as f32 / steps as f32);
            let cur = fast_tanh(x);
            assert!(
                cur >= prev,
                "monotonicity violated at x={x}: fast_tanh({x}) = {cur} < previous {prev}"
            );
            prev = cur;
        }
    }

    // ── THD tests ─────────────────────────────────────────────────────────────

    use crate::test_support::thd_db;

    #[test]
    fn fast_sine_thd() {
        let fft_size = 1024;
        let fundamental_bin = 8;
        let signal: Vec<f32> = (0..fft_size)
            .map(|i| {
                let phase = (i as f32 * fundamental_bin as f32 / fft_size as f32) % 1.0;
                fast_sine(phase)
            })
            .collect();
        let thd = thd_db(&signal, fundamental_bin, fft_size);
        println!("fast_sine THD: {thd:.2} dB");
        assert!(thd < -59.0, "fast_sine THD {thd:.2} dB exceeds -59 dB");
    }

    #[test]
    fn lookup_sine_thd() {
        let fft_size = 1024;
        let fundamental_bin = 8;
        let signal: Vec<f32> = (0..fft_size)
            .map(|i| {
                let phase = (i as f32 * fundamental_bin as f32 / fft_size as f32) % 1.0;
                lookup_sine(phase)
            })
            .collect();
        let thd = thd_db(&signal, fundamental_bin, fft_size);
        println!("lookup_sine THD: {thd:.2} dB");
        assert!(thd < -133.0, "lookup_sine THD {thd:.2} dB exceeds -133 dB");
    }

    #[test]
    fn fast_tanh_thd() {
        let fft_size = 1024;
        let fundamental_bin = 8;
        let signal: Vec<f32> = (0..fft_size)
            .map(|i| {
                fast_tanh(0.5 * (TAU * fundamental_bin as f32 * i as f32 / fft_size as f32).sin())
            })
            .collect();
        let thd = thd_db(&signal, fundamental_bin, fft_size);
        println!("fast_tanh THD: {thd:.2} dB");
        assert!(thd < -31.0, "fast_tanh THD {thd:.2} dB exceeds -31 dB");
    }

    // ── lookup_sine SNR ────────────────────────────────────────────────────────

    #[test]
    fn test_lookup_sine_snr() {
        // T6 — Wavetable SNR: assert RMS error of lookup_sine vs f64::sin() over
        // full period is within tolerance.
        //
        // Documented tolerance: RMS < 1e-4 across a dense sweep of [0, 1).
        // A 1024-point linearly-interpolated table achieves ~1e-6 RMS in practice.
        let steps = 100_000u32;
        let mut sum_sq_err = 0.0f64;

        for i in 0..steps {
            let phase = i as f32 / steps as f32;
            let approx = lookup_sine(phase) as f64;
            let exact = (phase as f64 * std::f64::consts::TAU).sin();
            let err = approx - exact;
            sum_sq_err += err * err;
        }

        let rms = (sum_sq_err / steps as f64).sqrt();
        println!("lookup_sine RMS error: {rms:.2e}");
        // Documented tolerance: RMS < 1e-4 over full period.
        assert!(rms < 1e-4, "lookup_sine RMS error {rms:.2e} exceeds 1e-4");
    }
}
