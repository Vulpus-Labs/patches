---
id: "0256"
title: ADSR end-to-end envelope shape test
priority: medium
created: 2026-04-02
---

## Summary

The ADSR tests verify ramp linearity (T5) and stability under rapid gate
toggling (T4), but no test checks the overall envelope shape for a given
parameter set. There is no assertion that the envelope reaches 1.0 at the end of
attack, holds at the sustain level during the sustain phase, or reaches 0.0 at
the end of release. A regression in the state machine could produce an envelope
that has linear ramps but the wrong shape.

## Acceptance criteria

- [ ] Test with known parameters (e.g. attack=0.01s, decay=0.02s, sustain=0.5,
      release=0.03s at 48kHz): trigger high, gate held, then gate released.
      Verify:
  - [ ] Peak value during attack reaches 1.0 (within 1e-3).
  - [ ] Value during sustain phase settles to the sustain level (within 1e-3).
  - [ ] Value at end of release reaches 0.0 (within 1e-3).
- [ ] Test that re-triggering during release restarts the attack from the current
      level (not from zero, not from 1.0).
- [ ] Test that a very short gate (trigger + immediate release, shorter than
      attack time) produces an envelope that peaks below 1.0 and then releases.
- [ ] All tests in `adsr.rs` unit test module.

## Notes

The existing T5 tests confirm that each ramp segment is linear, so the new tests
should focus on the *transitions* between segments and the overall shape, not on
re-checking linearity.
