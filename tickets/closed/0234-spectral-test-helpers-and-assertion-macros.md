---
id: "0234"
title: Spectral test helpers and assertion macros
priority: high
created: 2026-04-01
---

## Summary

Build the shared foundation that all other E045 tickets depend on. Add
frequency-domain test utilities to `patches-dsp/src/test_support.rs`:
helpers for extracting bin magnitudes from packed FFT output, computing a
magnitude response from an impulse response, finding the dominant bin, and
assertion macros for common spectral properties.

## Acceptance criteria

- [ ] `bin_magnitude(packed: &[f32], bin: usize) -> f32` — extracts the
      magnitude (hypot of real/imag) of bin `k` from `RealPackedFft` packed
      format, handling the DC (`[0]`) and Nyquist (`[1]`) special cases.

- [ ] `magnitude_spectrum(packed: &[f32], n: usize) -> Vec<f32>` — returns
      a `Vec<f32>` of length `n/2 + 1` with linear magnitudes for bins
      0 through N/2.

- [ ] `magnitude_response_db(impulse_response: &[f32], fft_size: usize) -> Vec<f32>`
      — zero-pads the impulse response to `fft_size`, runs a forward FFT,
      returns magnitudes in dB (20 log10) for bins 0 through N/2.
      `fft_size` must be a power of 2 ≥ 4.

- [ ] `dominant_bin(packed: &[f32], n: usize) -> usize` — returns the bin
      index (1..N/2-1, excluding DC and Nyquist) with the largest magnitude.
      Consolidated from the duplicated implementations in
      `spectral_pitch_shift.rs` and `tests/slot_deck.rs`.

- [ ] `assert_passband_flat!(response_db, bin_range, tolerance_db)` — asserts
      every bin in `bin_range` is within `±tolerance_db` of 0 dB. Panic
      message names the first offending bin and its dB value.

- [ ] `assert_stopband_below!(response_db, bin_range, threshold_db)` — asserts
      every bin in `bin_range` is below `threshold_db`. Panic message names
      the worst offending bin.

- [ ] `assert_peak_at_bin!(response_db_or_linear, expected_bin, tolerance_bins)`
      — asserts the dominant bin is within `±tolerance_bins` of `expected_bin`.

- [ ] `assert_slope_db_per_octave!(response_db, bin_range, sample_rate, expected_slope, tolerance)`
      — fits the dB-vs-log2(frequency) slope across `bin_range` using
      octave-band averaging or linear regression, asserts it is within
      `±tolerance` of `expected_slope`. Useful for pink (-3 dB/oct) and
      brown (-6 dB/oct) noise.

- [ ] All helpers and macros are `pub(crate)` and re-exported via
      `pub(crate) use` in `test_support.rs`.

- [ ] The duplicate `dominant_bin` implementations in `spectral_pitch_shift.rs`
      and `tests/slot_deck.rs` are replaced with calls to the shared helper.

- [ ] Existing FFT tests in `fft.rs` (`sine_concentrates_at_correct_bin`,
      `impulse_produces_flat_spectrum`) are updated to use `bin_magnitude`
      and/or `magnitude_spectrum` where it simplifies the code.

- [ ] Unit tests for each helper function (at least: `bin_magnitude` on a
      known packed buffer, `dominant_bin` on a synthetic spectrum,
      `magnitude_response_db` round-trips a known impulse to expected dB
      values).

- [ ] `cargo test -p patches-dsp` passes.
- [ ] `cargo clippy -- -D warnings` clean.

## Notes

All helpers are test-only (`#[cfg(test)]`) — they should not appear in release
builds.

`magnitude_response_db` should construct a `RealPackedFft` internally (it is
cheap for test-sized FFTs). Callers who need the raw packed buffer for further
manipulation can use `RealPackedFft` directly and pass the result to
`bin_magnitude` / `magnitude_spectrum`.

For `assert_slope_db_per_octave!`, octave-band averaging (mean dB in
[f, 2f) bands) is simpler than least-squares regression and sufficient for
noise spectral shape tests. The slope is `(mean_dB[band+1] - mean_dB[band])`
averaged over all adjacent octave-band pairs.
