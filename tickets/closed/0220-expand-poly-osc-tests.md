---
id: "0220"
title: Expand PolyOsc tests to match mono Oscillator coverage
priority: medium
created: 2026-03-30
---

## Summary

`poly_osc.rs` has 2 tests against `oscillator.rs`'s 9. The poly oscillator
exposes the same waveforms, pulse-width CV, and phase-modulation inputs as the
mono oscillator, but none of those paths are exercised at the module level. This
ticket brings `PolyOsc` test coverage up to feature-parity.

## Acceptance criteria

- [ ] A `make_poly_osc(sample_rate: f32, voices: usize)` factory helper is added,
  matching the pattern used in `oscillator.rs`, `adsr.rs`, and `lfo.rs`.
- [ ] `disconnected_outputs_are_not_written` — sentinel test: seed pool with
  `Poly([99.0; 16])`, disconnect all outputs, tick once, assert all voices still
  read 99.0 for every output port.
- [ ] `sine_output_correct_shape` — with `poly_voices = 1`, verify that at sample
  25 of a 100-sample C0 period the sine output for voice 0 is within 1e-3 of
  `lookup_sine(0.25)` (peak).
- [ ] `triangle_output_correct_shape` — analogous check for triangle waveform at
  phase 0.25 (peak) and 0.75 (trough).
- [ ] `square_polyblep_edges_smoothed` — verify PolyBLEP-corrected square output
  is not exactly ±1.0 at the rising and falling transitions, mirroring the mono
  test.
- [ ] `square_duty_cycle_responds_to_pulse_width_input` — with `pw = 1.0` CV
  (clamped to 0.99) on voice 0, verify approximately 99 positive samples per
  100-sample period for voice 0.
- [ ] `phase_mod_half_cycle_shifts_sine_output` — apply `phase_mod = 0.5` on
  voice 0 and verify the sine output matches `lookup_sine(0.5)`, mirroring the
  mono test.
- [ ] `voct_input_drives_independent_phases_per_voice` — already present; retain
  as-is (or refactor to use the new helper).
- [ ] `cargo test -p patches-modules` passes with 0 failures.
- [ ] `cargo clippy -p patches-modules` passes with 0 warnings.

## Notes

Use `poly_voices: 1` or `poly_voices: 2` for most new tests to keep assertions
simple. Only tests that are specifically about voice independence need more voices.

`assert_within!` tolerances should match the equivalent mono tests with a brief
comment (e.g. `// PolyBLEP correction is order-1; ~1% error at transitions is expected`).

The existing `sawtooth_output_not_zero_after_one_tick` test should be removed or
replaced as part of T-0219; do not duplicate effort here.
