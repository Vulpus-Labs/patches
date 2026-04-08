---
id: "0221"
title: Expand PolyAdsr tests to match mono Adsr coverage
priority: medium
created: 2026-03-30
---

## Summary

`poly_adsr.rs` has 4 tests against `adsr.rs`'s 5, and the existing 4 are
thinner than their mono counterparts. The `read_poly_voice_convenience` test
verifies harness behaviour rather than the module. Three state-machine cases
present in the mono suite — sustain hold, release slope, retrigger mid-release —
are entirely absent. This ticket brings the suite up to parity and removes the
misplaced harness test.

## Acceptance criteria

- [ ] `read_poly_voice_convenience` removed (it tests `ModuleHarness`, not
  `PolyAdsr`; the convenience method is already exercised by every other test).
- [ ] `sustain_holds_while_gate_high` — mirror the mono test: trigger voice 0,
  advance through attack and decay, hold gate high, verify the poly output for
  voice 0 is within 1e-3 of the configured sustain level across multiple ticks;
  voice 1 must remain at 0.0 throughout.
- [ ] `release_falls_to_zero` — mirror the mono test: bring voice 0 to sustain,
  then lower its gate, verify the envelope decreases by the expected per-sample
  decrement each tick; voice 1 must remain at 0.0.
- [ ] `retrigger_mid_release_restarts_attack` — mirror the mono test: put voice 0
  into release phase, apply a new trigger to voice 0, verify the envelope for
  voice 0 immediately jumps back to the attack phase (level = 1.0); voice 1 must
  remain at 0.0.
- [ ] `poly_output_clamped_to_unit_range` — run both voices with gate high for 20
  ticks; every voice value in the poly output must be in [0.0, 1.0].
- [ ] All existing passing tests (`idle_output_is_zero`, `attack_rises_on_trigger_for_single_voice`,
  `two_voices_are_independent`) are retained unchanged.
- [ ] `cargo test -p patches-modules` passes with 0 failures.
- [ ] `cargo clippy -p patches-modules` passes with 0 warnings.

## Notes

Use `sample_rate: 10.0` (as existing tests do) so ADSR time constants map to
round numbers of samples — this keeps the per-sample increment arithmetic
obvious in the test body.

The `arr(val, voice)` helper already present in the file is the right way to
construct per-voice trigger/gate arrays; reuse it.
