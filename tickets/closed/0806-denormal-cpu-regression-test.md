---
id: "0806"
title: Denormal CPU-cost regression test
priority: low
created: 2026-05-04
---

## Summary

Synthetic test that drives a long-tail patch (vreverb at decay
0.95, or vbbd with high feedback) into silence and measures
per-tick cost during the decay. Without mitigation, cost rises as
state hits subnormals; with FTZ + per-site flush (tickets 0802,
0804, 0805), cost stays flat.

## Acceptance criteria

- [ ] Lives in patches-integration-tests (or patches-profiling).
- [ ] Measures wall-clock per N-sample block during a decay tail of
      ~10 seconds after input cuts.
- [ ] Asserts ratio `cost_late_tail / cost_early_tail < threshold`
      (e.g. 1.5×). Without mitigation this ratio is typically
      10–50×.
- [ ] Runs under `just smoke` (not inner) — wall-clock test, noisy
      under load.
- [ ] Documents threshold rationale in test comments.

## Notes

- This test is the only way to catch denormal regressions: they are
  silent (output is correct) and only show as CPU.
- Run on a dedicated thread without FTZ to verify the test detects
  the problem; then enable FTZ and verify it passes.
