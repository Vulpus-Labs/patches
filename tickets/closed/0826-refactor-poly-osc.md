---
id: "0826"
title: Reduce poly_osc complexity and nesting
priority: medium
created: 2026-05-06
---

## Summary

[patches-modules/src/poly_osc.rs:200](../../patches-modules/src/poly_osc.rs#L200)
has cognitive complexity 37/25 — the highest in the workspace — and
`poly_osc.rs` accounts for 8 `excessive_nesting` hits clustered around
lines 316–340 (per-channel inner loops with branching on waveform / sync /
PM state).

Likely shape: a top-level `process` over channels with nested matches on
oscillator mode and sync source. Extract per-mode kernels (sine/saw/pulse
helpers or a small dispatch table) so the per-sample inner block is flat,
and lift waveform-selection out of the hot loop where the choice is
constant for the buffer.

## Acceptance criteria

- [ ] `process` (or whichever fn at line 200) ≤ cognitive 25
- [ ] No `excessive_nesting` warnings in `poly_osc.rs`
- [ ] Per-sample audio path remains allocation-free; benchmarks (if any
      exist for poly_osc) within noise
- [ ] `just commit -p patches-modules` clean

## Notes

Module testing strategy: prefer extracting pure inner kernels and testing
those rather than expanding the module-protocol surface.
