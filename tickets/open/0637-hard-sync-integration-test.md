---
id: "0637"
title: Hard-sync integration test and aliasing comparison
priority: medium
created: 2026-04-22
epic: "E103"
adr: "0047"
depends_on: ["0634", "0635", "0636"]
---

## Summary

End-to-end integration test in `patches-integration-tests` verifying
that a typed sub-sample sync chain produces less residual aliasing
than an equivalent threshold-detected sync chain (baseline obtained
by routing a 0/1 pulse through `TriggerToSync`).

## Acceptance criteria

- [ ] Test patch A: master `Osc` → `reset_out` → slave `Osc` `sync`
      (direct typed sync).
- [ ] Test patch B: master `Osc` saw-out → 0/1 pulse generator →
      `TriggerToSync` → slave `Osc` `sync` (simulates sample-boundary
      rounding).
- [ ] Render both at common sync ratios: 3:2, 2:1, golden (1.618),
      and a non-integer high ratio (7:2).
- [ ] Compute FFT of steady-state segment; measure aliasing energy
      above the Nyquist-of-master band. A > B not expected — B should
      have visibly higher residual aliasing.
- [ ] Test asserts B's aliasing floor exceeds A's by at least a
      documented margin (tuned once, hard-coded with a comment
      explaining the measurement).
- [ ] Same comparison wired through `VDco` / `VPolyDco` as a smoke
      test (not a gating assertion — vintage modules have their own
      character).
- [ ] `.patches` DSL fixture(s) under the existing fixtures
      directory, loadable via the standard integration-test harness.

## Notes

This test is the concrete answer to "does the ADR 0047 work actually
reduce aliasing?" Keep it deterministic (fixed seeds, fixed sample
rate, fixed duration). Document the measurement methodology in the
test file so future regressions are easy to diagnose.
