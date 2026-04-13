---
id: "0375"
title: Fix Bitcrusher CV stale-state on zero
priority: high
created: 2026-04-13
---

## Summary

In `patches-modules/src/bitcrusher.rs` lines 131–138, the `periodic_update`
method only applies CV when non-zero (`if rate_cv != 0.0`). When CV drops
back to zero, the kernel retains the last CV-modulated value rather than
reverting to the base parameter.

## Acceptance criteria

- [ ] Remove the `if rate_cv != 0.0` / `if depth_cv != 0.0` guards
- [ ] Always set the kernel to `base + cv` (clamped to valid range)
- [ ] Test: apply CV, remove CV, verify kernel returns to base value

## Notes

The fix is a one-line change per CV input: unconditionally apply `self.rate + rate_cv` and `self.depth + depth_cv`, clamped.
