/// Xorshift64 PRNG: maps the u64 state (reinterpreted as i64) to a float in [-1, 1].
///
/// # Precondition
/// `state` must be non-zero. If `state == 0`, the PRNG is stuck and will always
/// return `0.0`. Callers are responsible for ensuring a non-zero seed (e.g. by
/// adding 1 to an instance ID).
pub fn xorshift64(state: &mut u64) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state as i64 as f32) / (i64::MAX as f32)
}

/// 3-pole IIR pink noise shaping filter (Voss–McCartney / Kellett's method).
///
/// Feed white noise samples (in [-1, 1]) into `process()` to obtain
/// pink-shaped output with approximately -3 dB/octave roll-off.
/// Output range is approximately [-1, 1] for white input in [-1, 1].
pub struct PinkFilter {
    b0: f32,
    b1: f32,
    b2: f32,
}

impl PinkFilter {
    pub fn new() -> Self {
        Self { b0: 0.0, b1: 0.0, b2: 0.0 }
    }

    /// Zeroes all internal state.
    pub fn reset(&mut self) {
        self.b0 = 0.0;
        self.b1 = 0.0;
        self.b2 = 0.0;
    }

    /// Process one white noise sample and return a pink-shaped sample.
    pub fn process(&mut self, white: f32) -> f32 {
        self.b0 = 0.99765 * self.b0 + white * 0.0990460;
        self.b1 = 0.96300 * self.b1 + white * 0.2965164;
        self.b2 = 0.57000 * self.b2 + white * 1.0526913;
        (self.b0 + self.b1 + self.b2 + white * 0.1848) * 0.11
    }
}

impl Default for PinkFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Leaky integrator for brown/red noise shaping.
///
/// Feed white noise (or a previous `BrownFilter` output) into `process()` to
/// obtain a random-walk signal clamped to [-1, 1].
/// Brown noise (-6 dB/octave) is produced by integrating white noise.
/// Red noise (-9 dB/octave) is produced by integrating brown noise.
pub struct BrownFilter {
    pub state: f32,
}

impl BrownFilter {
    pub fn new() -> Self {
        Self { state: 0.0 }
    }

    /// Zeroes internal state.
    pub fn reset(&mut self) {
        self.state = 0.0;
    }

    /// Process one sample and return the new random-walk value.
    pub fn process(&mut self, input: f32) -> f32 {
        self.state += input * 0.02;
        self.state = self.state.clamp(-1.0, 1.0);
        self.state
    }
}

impl Default for BrownFilter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T7 — determinism: two xorshift64 instances with the same seed produce identical sequences.
    #[test]
    fn t7_same_seed_same_sequence() {
        use crate::test_support::assert_deterministic;

        // Dummy inputs — xorshift64 ignores them but the macro needs an iterable.
        let dummy: Vec<f32> = vec![0.0; 1000];

        assert_deterministic!(
            12345_u64,
            &dummy,
            |s: &mut u64, _x: f32| xorshift64(s)
        );
    }

    /// T7 — state reset determinism: resetting a PinkFilter produces the same output as a fresh one.
    #[test]
    fn t7_pink_filter_reset_determinism() {
        use crate::test_support::assert_reset_deterministic;

        let white_inputs: Vec<f32> = {
            let mut s: u64 = 99999;
            (0..64).map(|_| xorshift64(&mut s)).collect()
        };

        assert_reset_deterministic!(
            PinkFilter::new(),
            &white_inputs,
            |f: &mut PinkFilter, x: f32| f.process(x),
            |f: &mut PinkFilter| f.reset()
        );
    }

    /// T8 — edge-case: zero seed causes xorshift64 to be stuck at 0.
    ///
    /// This documents the precondition that callers must ensure a non-zero seed.
    /// Modules avoid this by adding 1 to the instance_id before using it as a seed.
    #[test]
    fn t8_zero_seed_behavior() {
        let mut state: u64 = 0;
        let out = xorshift64(&mut state);
        assert_eq!(out, 0.0, "xorshift64 with zero seed must return 0.0");
        assert_eq!(state, 0, "xorshift64 with zero seed must leave state at 0 (stuck)");
    }

    /// T10 — statistical: white noise variance is in the expected range for [-1, 1] output.
    #[test]
    fn t10_white_noise_approximately_flat_power() {
        let n = 65536_usize;
        let mut state: u64 = 42;
        let samples: Vec<f32> = (0..n).map(|_| xorshift64(&mut state)).collect();
        let mean = samples.iter().sum::<f32>() / n as f32;
        let variance = samples.iter().map(|&x| (x - mean) * (x - mean)).sum::<f32>() / n as f32;
        // Uniform distribution on [-1, 1] has variance 1/3 ≈ 0.333.
        // Allow generous bounds due to non-uniform xorshift output.
        assert!(
            variance >= 0.2 && variance <= 0.4,
            "white noise variance {variance} must be in [0.2, 0.4]"
        );
    }

    /// White noise should have an approximately flat magnitude spectrum.
    /// We use Welch averaging (4 segments of 2048 samples) to reduce variance.
    #[test]
    fn white_noise_flat_spectrum() {
        use crate::fft::RealPackedFft;
        use crate::test_support::magnitude_spectrum;

        let seg_size = 2048;
        let num_segments = 4;
        let total = seg_size * num_segments; // 8192
        let mut state: u64 = 42;
        let samples: Vec<f32> = (0..total).map(|_| xorshift64(&mut state)).collect();

        let fft = RealPackedFft::new(seg_size);
        let num_bins = seg_size / 2 + 1;
        let mut avg_power = vec![0.0f32; num_bins];

        for seg_idx in 0..num_segments {
            let mut buf = vec![0.0f32; seg_size];
            buf.copy_from_slice(&samples[seg_idx * seg_size..(seg_idx + 1) * seg_size]);
            fft.forward(&mut buf);
            let mags = magnitude_spectrum(&buf, seg_size);
            for (i, &m) in mags.iter().enumerate() {
                avg_power[i] += m * m;
            }
        }
        for p in avg_power.iter_mut() {
            *p /= num_segments as f32;
        }

        // Convert to dB, skip DC (bin 0) and Nyquist (last bin)
        let db: Vec<f32> = avg_power
            .iter()
            .map(|&p| 10.0 * p.max(1e-20).log10())
            .collect();

        // Compute mean dB across interior bins
        let interior = &db[1..num_bins - 1];
        let mean_db = interior.iter().sum::<f32>() / interior.len() as f32;

        // Assert no bin deviates more than ±12 dB from mean
        for (i, &val) in interior.iter().enumerate() {
            let bin = i + 1;
            assert!(
                (val - mean_db).abs() <= 12.0,
                "white noise spectrum not flat: bin {} is {:.2} dB, mean is {:.2} dB (deviation {:.2} dB)",
                bin,
                val,
                mean_db,
                val - mean_db,
            );
        }
    }

    /// Pink noise should have approximately -3 dB/octave slope.
    #[test]
    fn pink_noise_slope_minus_3db_per_octave() {
        use crate::fft::RealPackedFft;
        use crate::test_support::{assert_slope_db_per_octave, magnitude_spectrum};

        let n = 16384;
        let sample_rate = 44100.0f32;
        let mut state: u64 = 42;
        let mut pink = PinkFilter::new();
        let mut buf: Vec<f32> = (0..n).map(|_| pink.process(xorshift64(&mut state))).collect();

        let fft = RealPackedFft::new(n);
        fft.forward(&mut buf);
        let mags = magnitude_spectrum(&buf, n);
        let db: Vec<f32> = mags
            .iter()
            .map(|&m| 20.0 * m.max(1e-20).log10())
            .collect();

        // Bin for freq f = f * n / sample_rate
        // 200 Hz -> bin ~74, 4000 Hz -> bin ~1486
        let bin_lo = (200.0 * n as f32 / sample_rate).round() as usize;
        let bin_hi = (4000.0 * n as f32 / sample_rate).round() as usize;

        assert_slope_db_per_octave!(db, bin_lo..=bin_hi, sample_rate, n, -3.0, 2.0);
    }

    /// Brown noise should have approximately -6 dB/octave slope.
    #[test]
    fn brown_noise_slope_minus_6db_per_octave() {
        use crate::fft::RealPackedFft;
        use crate::test_support::{assert_slope_db_per_octave, magnitude_spectrum};

        let n = 16384;
        let sample_rate = 44100.0f32;
        let mut state: u64 = 42;
        let mut brown = BrownFilter::new();
        let mut buf: Vec<f32> = (0..n).map(|_| brown.process(xorshift64(&mut state))).collect();

        let fft = RealPackedFft::new(n);
        fft.forward(&mut buf);
        let mags = magnitude_spectrum(&buf, n);
        let db: Vec<f32> = mags
            .iter()
            .map(|&m| 20.0 * m.max(1e-20).log10())
            .collect();

        let bin_lo = (200.0 * n as f32 / sample_rate).round() as usize;
        let bin_hi = (4000.0 * n as f32 / sample_rate).round() as usize;

        assert_slope_db_per_octave!(db, bin_lo..=bin_hi, sample_rate, n, -6.0, 2.0);
    }

    /// T10 — statistical: pink noise has more low-frequency variation than high-frequency.
    ///
    /// Uses slow (low-freq) variance vs fast (high-freq) variance:
    ///   slow = variance of 8-sample boxcar averages
    ///   fast = variance of (sample[i] - sample[i-1])
    /// Pink noise rolls off at high frequencies, so slow > fast is expected.
    #[test]
    fn t10_pink_noise_lower_variance_at_high_freq() {
        let n = 16384_usize;
        let mut prng_state: u64 = 777;
        let mut pink = PinkFilter::new();
        let samples: Vec<f32> = (0..n)
            .map(|_| pink.process(xorshift64(&mut prng_state)))
            .collect();

        // Slow variance: variance of non-overlapping 8-sample boxcar averages.
        let window = 8_usize;
        let boxcar_avgs: Vec<f32> = samples
            .chunks(window)
            .map(|chunk| chunk.iter().sum::<f32>() / chunk.len() as f32)
            .collect();
        let avg_mean = boxcar_avgs.iter().sum::<f32>() / boxcar_avgs.len() as f32;
        let slow_variance = boxcar_avgs
            .iter()
            .map(|&x| (x - avg_mean) * (x - avg_mean))
            .sum::<f32>()
            / boxcar_avgs.len() as f32;

        // Fast variance: variance of first differences (high-frequency content).
        let diffs: Vec<f32> = samples.windows(2).map(|w| w[1] - w[0]).collect();
        let diff_mean = diffs.iter().sum::<f32>() / diffs.len() as f32;
        let fast_variance = diffs
            .iter()
            .map(|&x| (x - diff_mean) * (x - diff_mean))
            .sum::<f32>()
            / diffs.len() as f32;

        assert!(
            slow_variance > fast_variance * 1.5,
            "pink noise: slow variance ({slow_variance:.6}) must be > 1.5x fast variance ({fast_variance:.6}); \
             pink noise should have more low-frequency energy"
        );
    }
}
