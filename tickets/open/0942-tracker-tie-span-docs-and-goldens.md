---
id: "0942"
title: Manual update + golden audio tests for tie-spread rolls
priority: low
created: 2026-05-19
epic: E152
depends_on: ["0940"]
---

## Summary

Document the tie-spread roll in the manual's tracker chapter, and add
a golden audio test that exercises the feature end-to-end through the
audio engine (not just the pattern-player unit tests).

## Acceptance criteria

- [ ] `docs/src/` tracker / pattern documentation describes:
  - The dual meaning of tie (`~`): sustain (after plain step) vs.
    roll-continuation (after `*N` anchor).
  - A table of examples (`x*3`, `x*3 ~`, `x*5 ~ ~`) with timing
    diagrams or sample-offset tables.
  - Known limitations: swing across span uses anchor-tick interval;
    span truncates at row end; absorbed-tie cells don't fire
    independently.
- [ ] At least one golden patch under the engine integration test
      suite plays a tie-spread roll figure and the recorded audio is
      bit-identical across runs.
- [ ] Existing tracker-related golden patches still pass unchanged
      (regression guard for `x*N` single-tick behaviour).
- [ ] `just push` green.

## Notes

- Per the dev-loop memory, no doctests anywhere — the manual examples
  should be plain code fences, not testable snippets.
- The fusion-Phase-2 memory warns that audio goldens may be
  regenerated; if this ticket lands after ticket 0849, regenerate
  goldens *only* for the new tie-spread tests, not for unrelated
  patches.
