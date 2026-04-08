---
id: "0259"
title: PolySvfKernel and PolyPhaseAccumulator dedicated tests
priority: low
created: 2026-04-02
---

## Summary

`PolySvfKernel` has one test (`poly_kernel_matches_mono_kernel`) that checks
parity with the mono kernel. `PolyPhaseAccumulator` has no dedicated tests at
all. Both types are used in production modules for 16-voice polyphonic
processing, and bugs in voice indexing, phase wrapping, or state isolation would
not be caught by the existing mono tests.

## Acceptance criteria

### PolySvfKernel

- [ ] `poly_svf_voices_are_independent`: drive two voices with different
      frequencies, confirm their outputs diverge.
- [ ] `poly_svf_determinism`: two identical poly kernels produce bit-identical
      output (equivalent of mono `t7_determinism`).
- [ ] `poly_svf_reset`: reset one voice, confirm it returns to initial state
      while other voices are unaffected.

### PolyPhaseAccumulator

- [ ] `poly_phase_accumulator_wraps_per_voice`: each voice wraps independently
      at its own increment rate.
- [ ] `poly_phase_accumulator_matches_mono`: 16 mono accumulators at different
      increments produce the same phases as one poly accumulator.
- [ ] `poly_phase_accumulator_reset_voice`: resetting one voice does not affect
      others.
- [ ] `poly_phase_accumulator_determinism`: bit-identical output from two
      instances.

## Notes

The mono-parity tests are the highest value — they provide transitive coverage
from the existing mono test suites. The independence and reset tests add
confidence in voice isolation.
