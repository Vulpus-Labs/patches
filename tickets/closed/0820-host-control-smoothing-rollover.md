---
id: "0820"
title: Continue host-control smoothing across block boundaries when un-converged
priority: medium
created: 2026-05-05
epic: E135
depends_on: "0817"
---

## Summary

`HostControlScratch` currently freezes mid-ramp at block boundaries.
If a smoothed-kind event lands near the end of block N (target = 1.0,
last smoothed sample = 0.7) and block N+1 has no further events, the
ramp does not continue: the next block step-fills from the previous
smoothed tail (0.7), the smoothing mask is empty (no event arrived),
and the AoS row stays flat at 0.7 forever.

The intended behaviour is that the ramp continues toward the most
recent commanded value across blocks until convergence, then stops.

## Acceptance criteria

- [ ] Replace the single `tail: [f32; MAX_HOST_CONTROLS]` field with
      two arrays:
      - `last_target[ch]` — the most recent commanded value, used as
        the step-fill seed for non-impulse lanes;
      - `last_smoothed[ch]` — the AoS row's last sample, used as the
        smoothing pass's initial `y`.
- [ ] Track an additional persistent `pending_smooth_mask: u64` that
      holds bits for lanes where `last_smoothed != last_target`
      (within an `EPSILON ≈ 1e-5`).
- [ ] `prepare_block`:
      - Step-fill seeds non-impulse rows from `last_target[ch]`,
        not the smoothed tail;
      - After step-fill, refresh `last_target[ch]` from the SoA row's
        last sample for non-impulse lanes;
      - Smoothing pass runs on
        `active_smoothing_mask | pending_smooth_mask`;
      - At end of pass, refresh `pending_smooth_mask`: set the bit
        for any lane where `|last_smoothed - last_target| > EPSILON`,
        clear otherwise. Clear `active_smoothing_mask`.
- [ ] Zero-cost property preserved: when no events arrive *and* every
      lane has converged, `active_smoothing_mask | pending_smooth_mask
      == 0` and the smoothing pass is skipped entirely.
- [ ] Tests:
      - Event near end of block N → block N+1 with no events →
        AoS row continues converging, reaches target within
        `~5τ` total samples;
      - Latched (toggle) lane unaffected: events still produce hard
        steps across block boundaries;
      - Impulse (trigger) lane unaffected: zero-fill seed unchanged;
      - Empty pending + empty active → smoothing skipped (assert via
        the existing scratch state inspectors / a new test hook).
- [ ] `just inner -p patches-engine` passes.
- [ ] `just push` passes including determinism / hash-stability suite.

## Notes

- The bug was discussed during the 0817 review: smoothing currently
  only fires when an event arrives during the block, which freezes
  in-flight ramps at block boundaries. The fix splits the
  "commanded value" and "smoothed value" state, which the original
  shape conflated.
- `EPSILON` chosen to avoid lanes lingering in `pending_smooth_mask`
  forever due to denormal residue. `1e-5` is well below audible
  level for any reasonable parameter range.
- Live convergence costs one extra `for t in 0..block_size` strided
  AoS pass per pending lane per block until convergence. With ~5 ms τ
  and typical 256 / 1024 / 2048-frame buffers, ramps converge inside
  one or two blocks and the pending set returns to empty.
