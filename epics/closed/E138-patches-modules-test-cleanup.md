---
id: E138
title: patches-modules test cleanup (low-value test sweep)
status: open
created: 2026-05-07
---

## Summary

Survey of `patches-modules/src/` test code surfaced a cluster of tests that
add little protective value: tautological (recompute the implementation and
assert equality), module-shell boilerplate (descriptor reflection, internal
field pokes), redundant duplicates across mixer variants, and verbose
hardcoded-phase tests that could be parametric.

Follow-up to closed E042. The aim is not coverage maximisation but signal:
remove tests that survive every mutation and consolidate near-duplicates so
real failures are easier to spot.

## Tickets

- 0829 — drop tautological module tests (mono_to_poly, poly_to_mono, sah)
- 0830 — drop module-shell boilerplate tests (convolution_reverb descriptor, poly_filter delta-array)
- 0831 — consolidate master_sequencer sync-enum tests
- 0832 — merge oscillator PolyBLEP tests into parametric form
- 0833 — consolidate mixer mute/solo across mixer variants
- 0834 — prune redundant clock/lfo/adsr/kick/mixer-unity tests; tighten delay sync_ms tolerance

## Notes

Survey notes captured in chat 2026-05-07. Each ticket should run
`just inner -p patches-modules` and inspect mutants for the touched module
where practical (E117 mutation harness) — if a deletion changes the mutant
score meaningfully, reconsider.
