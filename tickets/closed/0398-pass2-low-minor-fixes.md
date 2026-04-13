---
id: "0398"
title: Pass 2 low — minor readability and redundant-allocation fixes
priority: low
created: 2026-04-13
---

## Summary

Minor pass-2 findings — readability and a couple of tiny off-thread
allocations worth cleaning up.

## Acceptance criteria

### L1 — Tidy `expect()` chain in execution_state

- [x] `execution_state.rs::rebuild` split into two named binding phases
      (`resolve` → `NonNull::new`). No behaviour change.

### L3 — Drop intermediate `Vec` in tracker-receiver filter

- [~] Re-examined: the existing chain collects into
      `HashSet<InstanceId>` in a single pipeline, not via an intermediate
      `Vec`. Original finding was a misread of the code. Dropped.

## Notes

Pass-2 review, findings L1, L3.

Dropped from scope:

- **L2** (subnormal flush helper): already covered by ticket 0393's M6
  (`flush_denormal` in `patches-dsp`).
- **L4** (index-loop vs iterator consistency): too broad for a single
  ticket and the current mix is mostly legitimate.
- **L5** (exhaustive-match enforcement for `ParameterValue`): trivial; will
  be caught on the next `#[non_exhaustive]` touch or compiler upgrade.
