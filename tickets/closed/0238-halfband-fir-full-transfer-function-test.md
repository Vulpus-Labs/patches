---
id: "0238"
title: HalfbandFir full transfer function test
priority: low
created: 2026-04-01
---

## Summary

The HalfbandFir tests measure passband gain and stopband attenuation at single
frequencies (0.125 and 0.35 normalised). An FFT of the decimator's impulse
response would characterise the full transfer function — passband ripple across
the entire passband, stopband rejection across the entire stopband, and
transition band steepness.

Depends on T-0234.

## Acceptance criteria

- [ ] New test `halfband_fir_full_transfer_function` in `halfband.rs`:
      - Construct a default `HalfbandFir`.
      - Feed a unit impulse through `process(1.0, 0.0)` then
        `process(0.0, 0.0)` for enough pairs to capture the full FIR length
        (≥ 33 taps / 2 = 17 pairs, use 32 for margin).
      - Zero-pad the collected output to a power-of-2 FFT size (e.g. 256).
      - Compute `magnitude_response_db`.
      - Assert passband (bins 0 through ~0.2 × N/2) within ±0.1 dB using
        `assert_passband_flat!`.
      - Assert stopband (bins ~0.3 × N/2 through N/2) below -60 dB using
        `assert_stopband_below!`.

- [ ] New test `halfband_interpolator_full_transfer_function`:
      - Same approach for `HalfbandInterpolator`: feed an impulse at the base
        rate, collect the oversampled output, FFT, assert passband/stopband.
      - Passband (0 to ~0.45 × base_rate) within ±0.5 dB.
      - Stopband (image band, ~0.55 × oversampled_rate to Nyquist) below
        -60 dB.

- [ ] The existing single-frequency tests (`passband_gain_near_unity`,
      `stopband_attenuation_at_least_60db`) are kept.

- [ ] `cargo test -p patches-dsp` passes.
- [ ] `cargo clippy -- -D warnings` clean.

## Notes

The HalfbandFir is a 33-tap FIR, so the impulse response is short and
well-defined. Zero-padding to 256 gives 128 frequency bins — more than enough
to characterise the passband/transition/stopband regions.

Note that `HalfbandFir::process(a, b)` decimates by 2 (two inputs, one
output). The impulse response of the decimator is the polyphase decomposition
of the FIR, not the full FIR tap sequence. The test should capture this
decimated impulse response, which is what the transfer function actually
represents from the perspective of a signal passing through the decimator.
