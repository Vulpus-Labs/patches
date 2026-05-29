---
id: "0980"
title: Planner coverage fill — replan/state threading, tracker indices, resolve variants
priority: low
created: 2026-05-29
---

## Summary

Close the remaining test holes once the pipeline is typed and split, so every
stage — including the top-level shell and the stateful replan paths — has direct
coverage. These are the stages the census flagged as integration-only or thin:
`Planner::build` / `build_full` (state threading, tracker-receiver indices),
multi-replan stability, and the poly/stereo `resolve_input_buffers` variants.

## Acceptance criteria

- [ ] `Planner::build_full` direct tests (with the minimal test registry):
  - [ ] State threads correctly across replans: surviving nodes keep
        `InstanceId` and pool slot; removed nodes tombstoned; new nodes installed.
  - [ ] Tracker-receiver index list is built from both surviving and freshly
        installed `ReceivesTrackerData` modules, sorted, and matches pool slots.
  - [ ] `tracker_data` attached when provided; absent otherwise.
  - [ ] Error from the builder propagates as `BuildError` (not a panic).
- [ ] Multi-replan stress: a sequence of N builds with adds/removes/param
      changes/topology changes keeps buffer + module slot allocation stable and
      leak-free (freelist accounting balances; no slot reused while live).
- [ ] `resolve_input_buffers` variants: poly→poly, poly→mono, stereo→stereo, and
      cable maps with offset/clip — beyond the mono/broadcast cases already
      covered.
- [ ] `just push` green; `just smoke` green if integration tests are touched.

## Notes

Part of epic **E160** (ADR 0081), phase P4 — the last ticket. Depends on 0977
(typed stages) and 0978 (injected `InstanceId` makes deterministic replan tests
tractable). After this, every stage in the pipeline has happy-path, edge-case,
and (where fallible) error-path direct tests, satisfying the E160 acceptance bar.
