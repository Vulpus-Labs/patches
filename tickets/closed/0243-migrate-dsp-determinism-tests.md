---
id: "0243"
title: Migrate DSP determinism/reset tests to shared macros
priority: medium
created: 2026-04-01
---

## Summary

Seven+ modules in `patches-dsp` have near-identical determinism/reset tests
(~150 lines total). The new `assert_deterministic!` and
`assert_reset_deterministic!` macros in `test_support.rs` capture this pattern.
Migrate existing tests to use them.

Additionally, `rms()` and `sine_rms_warmed()` are duplicated in
`tone_filter.rs`, `tap_feedback_filter.rs`, `tests/slot_deck.rs`, and
`tests/fft_lowpass.rs`. Replace with the canonical versions from
`test_support`.

## Acceptance criteria

### Determinism macro migration

For each file, replace the inline test body with a macro invocation:

- [ ] `adsr.rs` — `t7_reset_produces_identical_output` →
      `assert_reset_deterministic!`
- [ ] `biquad.rs` — `t7_determinism_after_reset` →
      `assert_reset_deterministic!`
- [ ] `noise.rs` — `t7_same_seed_same_sequence` → `assert_deterministic!`;
      `t7_pink_filter_reset_determinism` → `assert_reset_deterministic!`
- [ ] `oscillator.rs` — `t7_two_accumulators_same_increment_are_bit_identical`
      → `assert_deterministic!`;
      `t7_reset_produces_same_sequence_as_fresh_instance` →
      `assert_reset_deterministic!`
- [ ] `peak_window.rs` — `peak_window_determinism` → `assert_deterministic!`
- [ ] `svf.rs` — `t7_determinism` → `assert_deterministic!`
- [ ] `interpolator.rs` — `halfband_interpolator_determinism` →
      `assert_deterministic!`
- [ ] `tone_filter.rs` — `state_reset_produces_identical_output` →
      `assert_reset_deterministic!`
- [ ] `tap_feedback_filter.rs` — `state_reset_produces_identical_output` →
      `assert_reset_deterministic!`

### RMS helper migration

- [ ] `tone_filter.rs` — remove local `sine_rms()` and `sine_rms_warmed()`;
      use `test_support::sine_rms_warmed` with a closure.
- [ ] `tap_feedback_filter.rs` — remove local `sine_rms_warmed()`.
- [ ] `tests/slot_deck.rs` — remove local `rms()`; use `test_support::rms`.
- [ ] `tests/fft_lowpass.rs` — remove local `rms()`; use `test_support::rms`.

### Verification

- [ ] All tests pass, zero clippy warnings.

## Notes

Some tests (e.g. `delay_buffer_determinism`, `poly_delay_buffer_determinism`)
have multi-output read patterns that don't fit the simple macro signature.
These can stay as-is or be simplified on a case-by-case basis — don't force
the macro where it doesn't fit naturally.

The `assert_reset_deterministic!` macro expects a `$process` closure returning
a single `f32`. For tests where the processor returns a tuple or array (e.g.
`HalfbandInterpolator::process` returns `[f32; 2]`), adapt the closure to
return one channel, or keep the test inline if the macro doesn't fit.
