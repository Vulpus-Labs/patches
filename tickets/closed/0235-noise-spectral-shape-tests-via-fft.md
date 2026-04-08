---
id: "0235"
title: Noise spectral shape tests via FFT
priority: medium
created: 2026-04-01
---

## Summary

Replace or supplement the time-domain statistical noise tests in `noise.rs`
with FFT-based spectral shape assertions. The current tests use variance
proxies (boxcar averaging for low-freq, first-differences for high-freq) which
are indirect. FFT-based tests directly verify the defining spectral properties:
white noise has a flat power spectrum, pink noise rolls off at -3 dB/octave.

Depends on T-0234.

## Acceptance criteria

- [ ] New test `white_noise_flat_spectrum` in `noise.rs`:
      - Generate ≥ 8192 samples of white noise (xorshift64, fixed seed).
      - FFT the buffer and compute the magnitude spectrum.
      - Use `assert_passband_flat!` (or equivalent) to verify all bins are
        within a tolerance band of the mean power. Tolerance should account
        for statistical variation (±6 dB is reasonable for a single FFT frame;
        alternatively average multiple frames for tighter bounds).

- [ ] New test `pink_noise_slope_minus_3db_per_octave` in `noise.rs`:
      - Generate ≥ 16384 samples of pink-filtered noise (fixed seed).
      - FFT the buffer and compute the magnitude spectrum in dB.
      - Use `assert_slope_db_per_octave!` to verify the slope is -3 dB/octave
        ±1.5 dB across at least 3 octave bands (e.g. bins corresponding to
        ~200 Hz through ~1600 Hz at 44100 Hz sample rate).

- [ ] If a `BrownFilter` exists, add `brown_noise_slope_minus_6db_per_octave`
      with the same pattern asserting -6 dB/octave ±1.5 dB.

- [ ] The existing time-domain tests (`t10_white_noise_approximately_flat_power`,
      `t10_pink_noise_lower_variance_at_high_freq`) are kept — they test
      different properties (time-domain statistics vs spectral shape) and are
      not redundant.

- [ ] `cargo test -p patches-dsp` passes.
- [ ] `cargo clippy -- -D warnings` clean.

## Notes

Single-frame FFT power estimates are noisy. Two strategies to tighten bounds:

1. **Welch averaging** — split the signal into overlapping segments, FFT each,
   average the power spectra. More code but tighter tolerances.
2. **Generous single-frame tolerance** — simpler, accepts ±6 dB variation per
   bin. Sufficient to catch gross errors (e.g. filter not applied, wrong
   coefficient) without false positives.

Start with option 2; refine to option 1 only if tests are flaky.

For the slope assertion, octave-band averaging naturally smooths the noise:
the mean power across all bins in an octave band has much lower variance than
a single bin, so ±1.5 dB tolerance on the slope should be stable.
