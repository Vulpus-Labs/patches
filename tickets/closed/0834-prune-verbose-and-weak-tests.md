---
id: "0834"
title: Prune verbose/weak tests; tighten delay sync_ms tolerance
priority: low
created: 2026-05-07
epic: E138
---

## Summary

Cluster of low-value or weak-assertion tests:

- `clock.rs:262-274` `all_outputs_initialized_to_zero` — zero-init smoke;
  delete or fold into an existing test as a first-sample check.
- `lfo.rs:364-391` `random_output_holds_per_period_and_is_in_range` — three
  cycles with 1e-15 tolerance; effectively asserts the implementation
  against itself. Reduce to a single-cycle hold + range check.
- `adsr.rs:218-239` `sustain_holds_while_gate_high` — overlaps
  `release_falls_to_zero` setup; merge or delete.
- `kick.rs:304-350` `pitch_parameter_affects_output` — 44 lines of
  zero-cross counting as a frequency proxy; replace with a direct
  phase/frequency check or shrink to ordering assertion.
- `delay.rs:564-576` `*_sync_ms_*` — `<= 1` sample tolerance is loose;
  document why or tighten.

## Acceptance criteria

- [ ] Each listed test deleted, shrunk, or has its tolerance/assertion
      tightened with a comment justifying the bound
- [ ] `just inner -p patches-modules` green

## Notes

Where a verbose test is the only coverage of a real invariant, prefer
shrinking over deleting. Mutation testing (E117) on the touched modules
is a useful sanity check.
