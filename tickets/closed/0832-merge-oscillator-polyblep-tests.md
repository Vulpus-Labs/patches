---
id: "0832"
title: Merge oscillator PolyBLEP tests into parametric form
priority: low
created: 2026-05-07
epic: E138
---

## Summary

`patches-modules/src/oscillator.rs` has separate PolyBLEP smoothness tests
for sawtooth (≈line 368) and square-at-transition (≈392-406) that each
hardcode one or two phase points. Merge into a single test parametric
over waveform × phase ∈ {0, 0.25, 0.5, 0.75}.

## Acceptance criteria

- [ ] Single test covers both waveforms and at least four phases
- [ ] Existing invariant (output bounded, no exact ±1 at transition) still
      asserted
- [ ] `just inner -p patches-modules` green

## Notes

If the assertions differ meaningfully between waveforms (e.g. saw allows
±1 elsewhere; square does not), keep two helper functions but drive both
from one parametric harness.
