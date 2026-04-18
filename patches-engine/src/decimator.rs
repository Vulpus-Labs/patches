/// Halfband FIR decimator for 2× (and cascaded 4×/8×) oversampling.
///
/// # Design
///
/// Uses a symmetric halfband FIR filter. The centre tap is fixed at 0.5;
/// the non-zero off-centre taps are supplied at construction time. Every other
/// tap is zero by the halfband property, so only `taps.len()` multiply-adds
/// are needed per output sample (plus the centre tap).
///
/// # API
///
/// [`Decimator::push`] accepts one oversampled input sample at a time and
/// returns `None` for the first `factor − 1` calls of each group and
/// `Some(output)` on the final call.  For `OversamplingFactor::None` it
/// always returns `Some(x)`.
///
/// 4× and 8× are implemented by cascading two or three 2× stages respectively.
///
/// # Allocation
///
/// `HalfbandFir::new` allocates the delay line on the heap once.
/// No allocation occurs during `process` or `push`.
use crate::oversampling::OversamplingFactor;
use patches_dsp::HalfbandFir;

// ── X2Stage ───────────────────────────────────────────────────────────────────

/// Wraps `HalfbandFir` with a one-sample-at-a-time `push` API.
///
/// Collects two input samples, then calls `HalfbandFir::process` to
/// produce one output. Returns `None` on the first push of each pair and
/// `Some(output)` on the second.
pub(crate) struct X2Stage {
    filter: HalfbandFir,
    pending: Option<f32>,
}

impl X2Stage {
    fn new() -> Self {
        Self {
            filter: HalfbandFir::default(),
            pending: None,
        }
    }

    #[inline]
    fn push(&mut self, x: f32) -> Option<f32> {
        match self.pending.take() {
            None => {
                self.pending = Some(x);
                None
            }
            Some(first) => {
                Some(self.filter.process(first, x))
            }
        }
    }
}

// ── Decimator ─────────────────────────────────────────────────────────────────

/// Anti-aliasing decimator for the oversampling path.
///
/// Constructed once at engine start. `push` is called once per oversampled
/// inner tick and returns `Some(output)` every `factor` calls.
pub enum Decimator {
    /// 1× — pass every sample through unchanged.
    Passthrough,
    /// 2× — one halfband FIR stage.
    X2(X2Stage),
    /// 4× — two cascaded halfband FIR stages.
    X4(X2Stage, X2Stage),
    /// 8× — three cascaded halfband FIR stages.
    X8(X2Stage, X2Stage, X2Stage),
}

impl Decimator {
    /// Create a decimator for the given oversampling factor.
    pub fn new(factor: OversamplingFactor) -> Self {
        match factor {
            OversamplingFactor::None => Decimator::Passthrough,
            OversamplingFactor::X2 => Decimator::X2(X2Stage::new()),
            OversamplingFactor::X4 => Decimator::X4(X2Stage::new(), X2Stage::new()),
            OversamplingFactor::X8 => Decimator::X8(X2Stage::new(), X2Stage::new(), X2Stage::new()),
        }
    }

    /// Feed one oversampled input sample into the decimator.
    ///
    /// Returns `None` for the first `factor − 1` calls of each group and
    /// `Some(output)` on the final call. For `OversamplingFactor::None` always
    /// returns `Some(x)`.
    #[inline]
    pub fn push(&mut self, x: f32) -> Option<f32> {
        match self {
            Decimator::Passthrough => Some(x),
            Decimator::X2(s) => s.push(x),
            Decimator::X4(s1, s2) => s1.push(x).and_then(|y| s2.push(y)),
            Decimator::X8(s1, s2, s3) => {
                s1.push(x).and_then(|y| s2.push(y)).and_then(|z| s3.push(z))
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oversampling::OversamplingFactor;

    // ── Push-API behaviour tests ───────────────────────────────────────────────

    #[test]
    fn passthrough_always_returns_some() {
        let mut d = Decimator::new(OversamplingFactor::None);
        for i in 0..8u32 {
            assert!(d.push(i as f32).is_some(), "push {i} must return Some for passthrough");
        }
    }

    #[test]
    fn x2_alternates_none_some() {
        let mut d = Decimator::new(OversamplingFactor::X2);
        for i in 0..16u32 {
            let result = d.push(i as f32);
            if i % 2 == 0 {
                assert!(result.is_none(), "even push {i} must return None for X2");
            } else {
                assert!(result.is_some(), "odd push {i} must return Some for X2");
            }
        }
    }

    #[test]
    fn x4_returns_some_every_4th() {
        let mut d = Decimator::new(OversamplingFactor::X4);
        for i in 0..32u32 {
            let result = d.push(i as f32);
            if i % 4 == 3 {
                assert!(result.is_some(), "push {i} must return Some for X4");
            } else {
                assert!(result.is_none(), "push {i} must return None for X4");
            }
        }
    }

    #[test]
    fn x8_returns_some_every_8th() {
        let mut d = Decimator::new(OversamplingFactor::X8);
        for i in 0..64u32 {
            let result = d.push(i as f32);
            if i % 8 == 7 {
                assert!(result.is_some(), "push {i} must return Some for X8");
            } else {
                assert!(result.is_none(), "push {i} must return None for X8");
            }
        }
    }

    // ── Timing microbenchmark ─────────────────────────────────────────────────

    /// Measure the wall-clock cost of `Decimator::push` for each oversampling
    /// factor.  Run with `cargo test -p patches-engine -- bench_decimator_push
    /// --nocapture --ignored`.
    #[test]
    #[ignore]
    fn bench_decimator_push() {
        use std::hint::black_box;
        use std::time::Instant;

        const N: u64 = 4_000_000;   // inner-tick calls per factor
        const DEVICE_RATE: f64 = 44_100.0;

        for &(label, factor) in &[
            ("None (1×)", OversamplingFactor::None),
            ("X2  (2×)", OversamplingFactor::X2),
            ("X4  (4×)", OversamplingFactor::X4),
            ("X8  (8×)", OversamplingFactor::X8),
        ] {
            // Two decimators — left and right — matching AudioCallback.
            let mut dl = Decimator::new(factor);
            let mut dr = Decimator::new(factor);

            // Warm up.
            for i in 0..1024 {
                let _ = dl.push(i as f32 * 0.001);
                let _ = dr.push(i as f32 * 0.001);
            }

            let t0 = Instant::now();
            let f = factor.factor() as u64;
            // Simulate the AudioCallback inner loop: `f` inner ticks per output frame.
            let output_frames = N / f;
            for frame in 0..output_frames {
                for inner in 0..f {
                    let x = black_box((frame * f + inner) as f32 * 1e-6);
                    let _ = black_box(dl.push(x));
                    let _ = black_box(dr.push(x));
                }
            }
            let elapsed = t0.elapsed();

            let ns_per_inner   = elapsed.as_nanos() as f64 / N as f64;
            let ns_per_frame   = ns_per_inner * f as f64;
            let budget_ns      = 1_000_000_000.0 / DEVICE_RATE;
            let pct_of_budget  = ns_per_frame / budget_ns * 100.0;

            eprintln!(
                "{label}: {ns_per_inner:.2} ns/inner-tick  |  {ns_per_frame:.2} ns/output-frame  \
                 ({pct_of_budget:.2}% of {DEVICE_RATE:.0} Hz budget)"
            );
        }
    }

    // ── Audio-quality tests (X2 only) ─────────────────────────────────────────

    fn sine_samples(freq_hz: f32, sample_rate_hz: f32, n: usize) -> Vec<f32> {
        let two_pi = 2.0 * std::f32::consts::PI;
        (0..n)
            .map(|i| (two_pi * freq_hz * i as f32 / sample_rate_hz).sin())
            .collect()
    }

    fn collect_decimated_x2(input: &[f32]) -> Vec<f32> {
        let mut d = Decimator::new(OversamplingFactor::X2);
        let mut out = Vec::with_capacity(input.len() / 2 + 1);
        for &x in input {
            if let Some(y) = d.push(x) {
                out.push(y);
            }
        }
        out
    }

    fn rms(samples: &[f32]) -> f32 {
        let sum_sq: f32 = samples.iter().map(|&x| x * x).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }

    /// Discard this many output samples to let the filter settle.
    const SKIP: usize = 64;
    const N: usize = 96_000;

    /// 1 kHz at 96 kHz — well inside the passband of the 48 kHz half-band.
    /// Output amplitude must be within 0.1 dB of input amplitude.
    #[test]
    fn x2_passband_1khz_within_0_1_db() {
        let input = sine_samples(1_000.0, 96_000.0, N);
        let out = collect_decimated_x2(&input);
        let out = &out[SKIP.min(out.len())..];

        let in_rms = rms(&input);
        let out_rms = rms(out);
        let db = 20.0 * (out_rms / in_rms).log10();
        assert!(
            db.abs() < 0.1,
            "X2 passband 1 kHz: {db:.4} dB amplitude error exceeds 0.1 dB limit"
        );
    }

    /// 30 kHz at 96 kHz — above the 24 kHz Nyquist of the 48 kHz output.
    /// Output RMS must be at least 40 dB below full scale.
    #[test]
    fn x2_stopband_30khz_at_least_40_db() {
        let input = sine_samples(30_000.0, 96_000.0, N);
        let out = collect_decimated_x2(&input);
        let out = &out[SKIP.min(out.len())..];

        let in_rms = rms(&input);
        let out_rms = rms(out);
        let db = 20.0 * (out_rms / in_rms).log10();
        assert!(
            db < -40.0,
            "X2 stopband 30 kHz: {db:.1} dB is less than the required 40 dB attenuation"
        );
    }
}
