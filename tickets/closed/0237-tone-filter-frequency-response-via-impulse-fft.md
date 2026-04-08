---
id: "0237"
title: ToneFilter frequency response via impulse-response FFT
priority: medium
created: 2026-04-01
---

## Summary

The ToneFilter tests measure frequency response by running the filter with
sinusoids at individual frequencies (100 Hz, 1 kHz, 10 kHz), each requiring a
fresh filter instance and warmup period. An impulse-response FFT characterises
the full response shape in one pass, enabling richer assertions: the -3 dB
point location, rolloff slope, and passband flatness.

Depends on T-0234.

## Acceptance criteria

- [ ] New test `tone_one_flat_response_fft` in `tone_filter.rs`:
      - Construct a `ToneFilter`, prepare at 48 kHz, set tone = 1.0.
      - Feed a unit impulse, collect ≥ 512 samples of output.
      - Compute `magnitude_response_db`.
      - Assert passband (100 Hz–20 kHz) is flat within ±0.5 dB using
        `assert_passband_flat!`.

- [ ] New test `tone_zero_lowpass_shape_fft` in `tone_filter.rs`:
      - Construct a `ToneFilter`, prepare at 48 kHz, set tone = 0.0.
      - Feed a unit impulse, collect ≥ 1024 samples.
      - Compute `magnitude_response_db`.
      - Assert bins below 100 Hz are within ±3 dB of 0 dB (passband).
      - Assert bins above 5 kHz are below -20 dB (stopband).
      - Optionally assert rolloff slope is approximately -6 dB/octave
        (first-order filter) using `assert_slope_db_per_octave!` if the
        slope macro is available from T-0234.

- [ ] New test `tone_half_midpoint_shape_fft` in `tone_filter.rs`:
      - Tone = 0.5. Assert the -3 dB point is between 200 Hz and 2 kHz
        (the exact value depends on the filter topology — this test documents
        it rather than prescribing it).

- [ ] The existing sinusoid-driven tests are kept as independent cross-checks.

- [ ] `cargo test -p patches-dsp` passes.
- [ ] `cargo clippy -- -D warnings` clean.

## Notes

The ToneFilter is a simple one-pole filter, so its impulse response decays
quickly. A 512-point FFT at 48 kHz gives ~94 Hz bin resolution, which is
adequate for the tone=1.0 flatness test. For tone=0.0 where the response
has more structure at low frequencies, a 1024 or 2048-point FFT gives better
resolution (47 Hz or 23 Hz per bin).
