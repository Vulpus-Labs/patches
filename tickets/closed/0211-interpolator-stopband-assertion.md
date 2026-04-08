---
id: "0211"
title: Add stopband attenuation assertion to HalfbandInterpolator tests
priority: medium
created: 2026-03-30
---

## Summary

`patches-dsp/src/interpolator.rs` has a passband test (`passband_1khz_within_0_1_db`)
but no assertion that the stopband is actually attenuated. The halfband design
targets ≥ 60 dB stopband attenuation; this should be an automated assertion. Without
it, a regression that weakens the stopband (e.g. a coefficient change) would pass
all tests silently.

## Acceptance criteria

- [ ] A new test `stopband_is_attenuated_by_at_least_60_db` (or similar) in
      `patches-dsp/src/interpolator.rs` that:
      - Drives `HalfbandInterpolator` with a sinusoid well into the stopband at
        base rate (e.g. 20 kHz at 48 kHz base rate → 41.7% of base-rate Nyquist,
        which maps into the stopband of the halfband).
      - Measures steady-state output RMS after the group-delay settling period.
      - Asserts attenuation ≥ 60 dB relative to input amplitude.
- [ ] `cargo test -p patches-dsp` passes; `cargo clippy -p patches-dsp` clean.

## Notes

Technique references (ADR 0022): T2.

The stopband of a halfband filter begins at fs/4 (half of the base-rate Nyquist).
At 48 kHz base rate, fs/4 = 12 kHz. Any frequency above 12 kHz at base rate
should be in the stopband and attenuated by the filter spec (≥ 60 dB for this
design). Use 20 kHz as a conservative test frequency that is solidly in the
stopband.

Allow enough samples for the group delay to flush before measuring. The group
delay constant `GROUP_DELAY_BASE_RATE` is exported and can be used to determine
the settling period.
