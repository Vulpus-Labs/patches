---
id: "0219"
title: Remove redundant patches-modules tests superseded by patches-dsp
priority: low
created: 2026-03-30
---

## Summary

Several tests in `patches-modules` verify properties that are now directly
guaranteed by `patches-dsp` tests added in E041. Retaining them increases
maintenance cost without adding safety. This ticket removes or replaces the
redundant assertions.

## Acceptance criteria

- [ ] `adsr::tests::output_clamped_to_unit_range` removed. The equivalent
  property is covered by `patches-dsp::adsr::t4_rapid_gate_toggling_no_nan_or_out_of_range`.
- [ ] `oscillator::tests::sine_output_completes_full_cycle_in_period_samples`
  replaced with an assertion on waveform shape (peak amplitude, zero-crossing
  count) rather than cycle-to-cycle identity. The cycle-consistency property is
  covered by `patches-dsp::oscillator::t7_*` determinism tests.
- [ ] `oscillator::tests::triangle_output_completes_full_cycle` similarly
  replaced with a shape assertion (peak at quarter-period, zero at half-period).
- [ ] `poly_osc::tests::sawtooth_output_not_zero_after_one_tick` either removed
  (finiteness is implied by the DSP-level PolyBLEP tests) or replaced with a
  meaningful shape assertion.
- [ ] `cargo test -p patches-modules` passes with 0 failures.
- [ ] `cargo clippy -p patches-modules` passes with 0 warnings.

## Notes

When replacing a cycle-consistency test with a shape test, use the same
`C0_FREQ * period` sample-rate trick already present in the file so that exact
sample indices correspond to known phase positions (e.g. sample 25 of a
100-sample period = phase 0.25 = peak of sine).

The `assert_within!` tolerance for peak amplitude should be `1e-3` (matching the
existing PolyBLEP tests in the same file) with a comment explaining the source
of error.
