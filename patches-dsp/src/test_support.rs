/// Assert that `actual` is within an absolute `delta` of `expected`.
///
/// Mirrors the `assert_within!` macro in `patches-core`, kept local so
/// `patches-dsp` stays dependency-free.
macro_rules! assert_within {
    ($expected:expr, $actual:expr, $delta:expr) => {{
        let expected: f32 = $expected;
        let actual: f32 = $actual;
        let delta: f32 = $delta;
        assert!(
            (expected - actual).abs() < delta,
            "assert_within failed: expected {}, actual {}, delta {}",
            expected,
            actual,
            delta
        );
    }};
    ($expected:expr, $actual:expr, $delta:expr, $($arg:tt)+) => {{
        let expected: f32 = $expected;
        let actual: f32 = $actual;
        let delta: f32 = $delta;
        assert!(
            (expected - actual).abs() < delta,
            $($arg)+
        );
    }};
}

pub(crate) use assert_within;

// ── Spectral helper functions ────────────────────────────────────────────────

use crate::fft::RealPackedFft;

/// Extract the magnitude of frequency bin `bin` from a packed FFT buffer.
///
/// DC (bin 0) → `packed[0].abs()`, Nyquist (bin N/2) → `packed[1].abs()`,
/// interior bins k ∈ 1..N/2-1 → `hypot(packed[2k], packed[2k+1])`.
pub(crate) fn bin_magnitude(packed: &[f32], bin: usize) -> f32 {
    if bin == 0 {
        packed[0].abs()
    } else if bin == packed.len() / 2 {
        packed[1].abs()
    } else {
        packed[2 * bin].hypot(packed[2 * bin + 1])
    }
}

/// Return a Vec of linear magnitudes for bins 0 through N/2.
///
/// `n` is the FFT size; the returned Vec has length `n/2 + 1`.
pub(crate) fn magnitude_spectrum(packed: &[f32], n: usize) -> Vec<f32> {
    let half = n / 2;
    (0..=half).map(|k| bin_magnitude(packed, k)).collect()
}

/// Zero-pad `impulse_response` to `fft_size`, run a forward FFT, and return
/// dB magnitudes (20·log10) for bins 0..N/2.
///
/// Returns a Vec of length `fft_size/2 + 1`.
pub(crate) fn magnitude_response_db(impulse_response: &[f32], fft_size: usize) -> Vec<f32> {
    let fft = RealPackedFft::new(fft_size);
    let mut buf = vec![0.0f32; fft_size];
    let len = impulse_response.len().min(fft_size);
    buf[..len].copy_from_slice(&impulse_response[..len]);
    fft.forward(&mut buf);
    magnitude_spectrum(&buf, fft_size)
        .iter()
        .map(|&mag| 20.0 * mag.max(1e-20).log10())
        .collect()
}

/// Return the bin index (searching 1..N/2-1, excluding DC and Nyquist) with
/// the largest magnitude in a packed FFT buffer. `n` is the FFT size.
pub(crate) fn dominant_bin(packed: &[f32], n: usize) -> usize {
    let mut best_k = 1;
    let mut best_mag = 0.0f32;
    for k in 1..(n / 2) {
        let mag = bin_magnitude(packed, k);
        if mag > best_mag {
            best_mag = mag;
            best_k = k;
        }
    }
    best_k
}

/// Compute Total Harmonic Distortion in dB for a signal of exactly `fft_size`
/// samples with its fundamental at `fundamental_bin`.
///
/// Returns `10 * log10(harmonic_power / fundamental_power)` where harmonic_power
/// is the sum of squared magnitudes at bins 2×, 3×, 4×, ... × fundamental up to
/// Nyquist.
pub(crate) fn thd_db(signal: &[f32], fundamental_bin: usize, fft_size: usize) -> f32 {
    assert_eq!(signal.len(), fft_size);
    let fft = RealPackedFft::new(fft_size);
    let mut buf = vec![0.0f32; fft_size];
    buf.copy_from_slice(signal);
    fft.forward(&mut buf);

    let fund_mag = bin_magnitude(&buf, fundamental_bin);
    let fund_power = fund_mag * fund_mag;

    let nyquist = fft_size / 2;
    let mut harmonic_power = 0.0f32;
    let mut harmonic = 2 * fundamental_bin;
    while harmonic <= nyquist {
        let mag = bin_magnitude(&buf, harmonic);
        harmonic_power += mag * mag;
        harmonic += fundamental_bin;
    }

    10.0 * (harmonic_power / fund_power).log10()
}

// ── Assertion macros ─────────────────────────────────────────────────────────

/// Assert every bin in `bin_range` of `response_db` is within ±`tolerance_db`
/// of 0 dB. Panics naming the first offending bin and its dB value.
#[allow(unused_macros)]
macro_rules! assert_passband_flat {
    ($response_db:expr, $bin_range:expr, $tolerance_db:expr) => {{
        let response: &[f32] = &$response_db;
        let tolerance: f32 = $tolerance_db;
        for bin in $bin_range {
            let db = response[bin];
            assert!(
                db.abs() <= tolerance,
                "assert_passband_flat failed: bin {} is {:.2} dB (tolerance ±{:.1} dB)",
                bin,
                db,
                tolerance
            );
        }
    }};
}

#[allow(unused_imports)]
pub(crate) use assert_passband_flat;

/// Assert every bin in `bin_range` of `response_db` is below `threshold_db`
/// (typically a negative value like -60.0). Panics naming the worst offending
/// bin.
#[allow(unused_macros)]
macro_rules! assert_stopband_below {
    ($response_db:expr, $bin_range:expr, $threshold_db:expr) => {{
        let response: &[f32] = &$response_db;
        let threshold: f32 = $threshold_db;
        for bin in $bin_range {
            let db = response[bin];
            assert!(
                db <= threshold,
                "assert_stopband_below failed: bin {} is {:.2} dB (threshold {:.1} dB)",
                bin,
                db,
                threshold
            );
        }
    }};
}

#[allow(unused_imports)]
pub(crate) use assert_stopband_below;

/// Assert the index of the maximum value in `spectrum` (a slice) is within
/// ±`tolerance_bins` of `expected_bin`.
#[allow(unused_macros)]
macro_rules! assert_peak_at_bin {
    ($spectrum:expr, $expected_bin:expr, $tolerance_bins:expr) => {{
        let spectrum: &[f32] = &$spectrum;
        let expected: usize = $expected_bin;
        let tolerance: usize = $tolerance_bins;
        let peak = spectrum
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .expect("empty spectrum");
        let diff = if peak >= expected { peak - expected } else { expected - peak };
        assert!(
            diff <= tolerance,
            "assert_peak_at_bin failed: peak at bin {} (expected {} ± {})",
            peak,
            expected,
            tolerance
        );
    }};
}

#[allow(unused_imports)]
pub(crate) use assert_peak_at_bin;

/// Assert that the average dB/octave slope across `bin_range` is within
/// ±`tolerance` of `expected_slope`.
///
/// Computes octave-band averages, measures dB differences between adjacent
/// octave bands, and checks the mean slope.
#[allow(unused_macros)]
macro_rules! assert_slope_db_per_octave {
    ($response_db:expr, $bin_range:expr, $sample_rate:expr, $fft_size:expr, $expected_slope:expr, $tolerance:expr) => {{
        let response: &[f32] = &$response_db;
        let sample_rate: f32 = $sample_rate as f32;
        let fft_size: usize = $fft_size;
        let expected_slope: f32 = $expected_slope;
        let tolerance: f32 = $tolerance;

        let bin_to_freq = |b: usize| -> f32 { b as f32 * sample_rate / fft_size as f32 };

        // Collect octave-band averages starting from the lowest bin in range.
        let range_start = {
            let mut s = 0usize;
            for b in $bin_range.clone() {
                s = b;
                break;
            }
            s
        };
        let range_end = {
            let mut e = 0usize;
            for b in $bin_range.clone() {
                e = b;
            }
            e
        };

        let mut octave_bands: Vec<(f32, f32)> = Vec::new(); // (centre_freq, mean_db)
        let mut lo = range_start.max(1);
        while lo <= range_end {
            let hi = (lo * 2).min(range_end + 1);
            if hi <= lo {
                break;
            }
            let sum: f32 = (lo..hi)
                .filter(|&b| b < response.len())
                .map(|b| response[b])
                .sum();
            let count = (lo..hi).filter(|&b| b < response.len()).count();
            if count > 0 {
                let centre = bin_to_freq((lo + hi) / 2);
                octave_bands.push((centre, sum / count as f32));
            }
            lo = hi;
        }

        assert!(
            octave_bands.len() >= 2,
            "assert_slope_db_per_octave: need at least 2 octave bands, got {}",
            octave_bands.len()
        );

        let mut slopes = Vec::new();
        for pair in octave_bands.windows(2) {
            let db_diff = pair[1].1 - pair[0].1;
            let octave_diff = (pair[1].0 / pair[0].0).log2();
            if octave_diff > 0.0 {
                slopes.push(db_diff / octave_diff);
            }
        }

        assert!(
            !slopes.is_empty(),
            "assert_slope_db_per_octave: could not compute any octave slopes"
        );

        let mean_slope: f32 = slopes.iter().sum::<f32>() / slopes.len() as f32;
        assert!(
            (mean_slope - expected_slope).abs() <= tolerance,
            "assert_slope_db_per_octave failed: mean slope {:.2} dB/oct (expected {:.1} ± {:.1})",
            mean_slope,
            expected_slope,
            tolerance
        );
    }};
}

#[allow(unused_imports)]
pub(crate) use assert_slope_db_per_octave;

// ── Signal generation ────────────────────────────────────────────────────────

/// Generate `n` samples of a sine wave at `freq_hz` given `sample_rate`.
pub(crate) fn sine_signal(freq_hz: f32, sample_rate: f32, n: usize) -> Vec<f32> {
    use std::f32::consts::TAU;
    let inc = freq_hz / sample_rate;
    (0..n).map(|i| (i as f32 * inc * TAU).sin()).collect()
}

// ── RMS measurement ─────────────────────────────────────────────────────────

/// Root-mean-square of a signal slice.
pub(crate) fn rms(signal: &[f32]) -> f32 {
    let sum_sq: f32 = signal.iter().map(|&x| x * x).sum();
    (sum_sq / signal.len() as f32).sqrt()
}

/// Measure steady-state RMS of a sine wave processed through a filter.
///
/// Generates a sine at `freq_hz`, discards `warmup` samples to let the filter
/// settle, then measures RMS over `measure` samples.
///
/// `process` is called once per sample with the input value and must return the
/// filter's output.
pub(crate) fn sine_rms_warmed(
    freq_hz: f32,
    sample_rate: f32,
    warmup: usize,
    measure: usize,
    mut process: impl FnMut(f32) -> f32,
) -> f32 {
    use std::f32::consts::TAU;
    let inc = freq_hz / sample_rate;
    for i in 0..warmup {
        let x = (i as f32 * inc * TAU).sin();
        process(x);
    }
    let sum_sq: f32 = (warmup..warmup + measure)
        .map(|i| {
            let x = (i as f32 * inc * TAU).sin();
            let y = process(x);
            y * y
        })
        .sum();
    (sum_sq / measure as f32).sqrt()
}

// ── Determinism testing ─────────────────────────────────────────────────────

/// Assert that two runs of a DSP processor on identical input produce
/// bit-identical output.
///
/// `$make` is an expression producing a fresh processor instance.
/// `$input` is an iterable of input values.
/// `$process` is a closure `|processor: &mut _, input| -> output` that advances
/// the processor by one sample.
///
/// # Example
///
/// ```ignore
/// assert_deterministic!(
///     SvfKernel::new_static(f, d),
///     &input_samples,
///     |k: &mut SvfKernel, x: f32| k.tick(x)
/// );
/// ```
#[allow(unused_macros)]
macro_rules! assert_deterministic {
    ($make:expr, $input:expr, $process:expr) => {{
        let mut a = $make;
        let mut b = $make;
        let process_fn = $process;
        for (i, input) in $input.iter().enumerate() {
            let out_a = process_fn(&mut a, *input);
            let out_b = process_fn(&mut b, *input);
            assert_eq!(
                out_a.to_bits(),
                out_b.to_bits(),
                "determinism violation at sample {}: {} vs {}",
                i,
                out_a,
                out_b
            );
        }
    }};
}

#[allow(unused_imports)]
pub(crate) use assert_deterministic;

/// Assert that resetting a DSP processor and re-running it produces
/// bit-identical output to a fresh instance.
///
/// `$make` is an expression producing a fresh processor instance.
/// `$input` is an iterable of input values.
/// `$process` is a closure `|processor: &mut _, input| -> output`.
/// `$reset` is a closure `|processor: &mut _|` that resets internal state.
///
/// # Example
///
/// ```ignore
/// assert_reset_deterministic!(
///     PinkFilter::new(),
///     &white_samples,
///     |f: &mut PinkFilter, x: f32| f.process(x),
///     |f: &mut PinkFilter| f.reset()
/// );
/// ```
#[allow(unused_macros)]
macro_rules! assert_reset_deterministic {
    ($make:expr, $input:expr, $process:expr, $reset:expr) => {{
        let process_fn = $process;
        let reset_fn = $reset;

        // First run — use and then reset.
        let mut processor = $make;
        let mut first_run = Vec::new();
        for input in $input.iter() {
            first_run.push(process_fn(&mut processor, *input));
        }
        // Dirty the state further, then reset.
        for input in $input.iter() {
            process_fn(&mut processor, *input);
        }
        reset_fn(&mut processor);

        // Second run after reset.
        let mut second_run = Vec::new();
        for input in $input.iter() {
            second_run.push(process_fn(&mut processor, *input));
        }

        // Fresh instance for comparison.
        let mut fresh = $make;
        let mut fresh_run = Vec::new();
        for input in $input.iter() {
            fresh_run.push(process_fn(&mut fresh, *input));
        }

        for (i, ((a, b), c)) in first_run
            .iter()
            .zip(second_run.iter())
            .zip(fresh_run.iter())
            .enumerate()
        {
            assert_eq!(
                a.to_bits(),
                c.to_bits(),
                "first run vs fresh instance differs at sample {}: {} vs {}",
                i, a, c
            );
            assert_eq!(
                b.to_bits(),
                c.to_bits(),
                "reset run vs fresh instance differs at sample {}: {} vs {}",
                i, b, c
            );
        }
    }};
}

#[allow(unused_imports)]
pub(crate) use assert_reset_deterministic;

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_magnitude_dc_and_nyquist() {
        // Packed buffer for N=8: DC, Nyquist, then bins 1..3 (real, imag pairs)
        let packed = [2.0f32, -3.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        // DC = |packed[0]| = 2.0
        assert_within!(2.0, bin_magnitude(&packed, 0), 1e-6);
        // Nyquist (bin 4 = N/2 = 8/2) = |packed[1]| = 3.0
        assert_within!(3.0, bin_magnitude(&packed, 4), 1e-6);
    }

    #[test]
    fn bin_magnitude_interior() {
        // Packed buffer for N=8: DC=0, Nyq=0, bin1=(3,4), bin2=(0,0), bin3=(0,0)
        let packed = [0.0f32, 0.0, 3.0, 4.0, 0.0, 0.0, 0.0, 0.0];
        // bin 1: hypot(3, 4) = 5
        assert_within!(5.0, bin_magnitude(&packed, 1), 1e-6);
        // bin 2: hypot(0, 0) = 0
        assert_within!(0.0, bin_magnitude(&packed, 2), 1e-6);
    }

    #[test]
    fn dominant_bin_finds_strongest() {
        // N=16, so packed has 16 values: DC, Nyq, then bins 1..7
        let mut packed = [0.0f32; 16];
        // Put strong signal in bin 5: packed[10] = re, packed[11] = im
        packed[10] = 10.0;
        packed[11] = 0.0;
        // Put weaker signal in bin 3
        packed[6] = 1.0;
        packed[7] = 1.0;

        assert_eq!(5, dominant_bin(&packed, 16));
    }

    #[test]
    fn magnitude_response_db_unit_impulse_is_flat() {
        let fft_size = 64;
        // Unit impulse: [1, 0, 0, ...] → flat magnitude spectrum at 0 dB
        let impulse = [1.0f32];
        let db = magnitude_response_db(&impulse, fft_size);
        assert_eq!(db.len(), fft_size / 2 + 1);
        for (bin, &val) in db.iter().enumerate() {
            assert!(
                val.abs() < 0.1,
                "unit impulse should be ~0 dB everywhere, but bin {} is {:.2} dB",
                bin,
                val
            );
        }
    }

    // ── Signal generation tests ──────────────────────────────────────────────

    #[test]
    fn sine_signal_length_and_range() {
        let sig = sine_signal(440.0, 44100.0, 100);
        assert_eq!(sig.len(), 100);
        for &v in &sig {
            assert!(v >= -1.0 && v <= 1.0, "sine sample out of range: {v}");
        }
    }

    // ── RMS tests ────────────────────────────────────────────────────────────

    #[test]
    fn rms_of_constant_signal() {
        let sig = vec![3.0f32; 100];
        assert_within!(3.0, rms(&sig), 1e-6);
    }

    #[test]
    fn rms_of_sine_is_roughly_inv_sqrt2() {
        let sig = sine_signal(100.0, 44100.0, 44100);
        // RMS of a sine wave = 1/√2 ≈ 0.7071
        assert_within!(std::f32::consts::FRAC_1_SQRT_2, rms(&sig), 0.01);
    }

    #[test]
    fn sine_rms_warmed_passthrough() {
        // Identity filter: output = input. Warmed RMS should match raw sine RMS.
        let measured = sine_rms_warmed(100.0, 44100.0, 1000, 44100, |x| x);
        assert_within!(std::f32::consts::FRAC_1_SQRT_2, measured, 0.01);
    }

    // ── Determinism macro tests ──────────────────────────────────────────────

    #[test]
    fn assert_deterministic_passes_for_pure_function() {
        let input = [1.0f32, 2.0, 3.0, 4.0];
        // A stateless "processor" that just doubles.
        assert_deterministic!((), &input, |_: &mut (), x: f32| x * 2.0);
    }

    #[test]
    fn assert_reset_deterministic_passes_for_accumulator() {
        struct Acc(f32);
        impl Acc {
            fn tick(&mut self, x: f32) -> f32 { self.0 += x; self.0 }
            fn reset(&mut self) { self.0 = 0.0; }
        }
        let input = [1.0f32, 2.0, 3.0];
        assert_reset_deterministic!(
            Acc(0.0),
            &input,
            |a: &mut Acc, x: f32| a.tick(x),
            |a: &mut Acc| a.reset()
        );
    }
}
