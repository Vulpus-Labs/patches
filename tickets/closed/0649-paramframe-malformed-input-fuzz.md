---
id: "0649"
title: ParamFrame malformed-input fuzz (size/hash/tail)
priority: high
created: 2026-04-23
epic: "E111"
---

## Summary

Fuzz the `ParamFrame` decode path with malformed inputs: wrong byte
size, wrong `ParamLayout` hash, corrupted tail bytes. Every
malformed frame must be rejected; none may be partially decoded.

## Acceptance criteria

- [ ] Fuzz target (cargo-fuzz or proptest) covering frame size,
      layout hash mismatch, and tail corruption.
- [ ] Rejection is total: no getter returns a value from a frame
      that fails validation.
- [ ] CI runs a time-boxed pass; nightly gets a longer budget.
- [ ] No `unwrap`/`expect` added to library code.

## Notes

ADR 0045 §Spike 9. Pair with 0650 (`ArcTable` fuzz) and feed into
0651 (soak).
