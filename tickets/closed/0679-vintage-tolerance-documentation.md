---
id: "0679"
title: patches-vintage — document or tighten tolerance bands
priority: low
created: 2026-04-24
epic: E116
---

## Summary

Several patches-vintage DSP tests use unexplained magic thresholds
that could hide factor-of-10 regressions. Each threshold should
either be documented (derivation, source of number, why this tolerance
is correct) or tightened to a value that actually constrains
behaviour.

## Acceptance criteria

- [ ] `vdco/tests.rs:423-470` — document derivation of `mag < 0.5 *
      cur_fund` alias-bin threshold (or tighten).
- [ ] `vdco/tests.rs:599-683` — document the 85% naive-baseline ratio
      in `sync_aliasing_below_naive_baseline`.
- [ ] `vchorus/tests.rs:80-108` — `hiss peak > 0.0 && < 0.1` currently
      spans two orders of magnitude; tighten to the real expected band
      or replace with spectral check.
- [ ] `vflanger/tests.rs:27-35` — `silent_input_bounded_output` checks
      only `abs < 0.5`; tighten to expected silent-path bound (likely
      1e-6 ish) or rename to reflect what it actually asserts.

## Notes

Low priority — these are unlikely to fire on real regressions, but
when they do, the diagnostic is weak ("some DSP number moved"). A
one-line comment per threshold with the source (paper, bench
measurement, chosen conservatively because X) pays for itself the
next time someone hits a failure.
