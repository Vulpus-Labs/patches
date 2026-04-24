---
id: "0682"
title: Mutation testing — patches-dsp
priority: high
created: 2026-04-24
epic: E117
---

## Summary

Highest-priority crate for this epic. DSP kernels are arithmetic-heavy;
boundary / sign / comparator mutants tend to map to real bugs. Run
`cargo mutants -p patches-dsp` and triage.

## Acceptance criteria

- [ ] Run completes; counts recorded.
- [ ] Top-5 MISSED-ratio files listed (expect biquad/SVF, halfband,
      delay, ADSR, noise shaping as candidates).
- [ ] Follow-up tickets filed per hotspot, prioritised by audio-path
      impact.

## Notes

Depends on 0680. Long runtime expected — may run in background.
