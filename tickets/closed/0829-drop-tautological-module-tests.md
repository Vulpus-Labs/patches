---
id: "0829"
title: Drop tautological tests in mono_to_poly, poly_to_mono, sah
priority: low
created: 2026-05-07
epic: E138
---

## Summary

Three tests recompute the module's logic in the test body and assert the
result equals what the module produced. They cannot fail unless the
harness itself breaks.

- `patches-modules/src/mono_to_poly.rs:85-96` — writes mono value, reads it
  back from each of 16 channels; the loop in the test mirrors the module.
- `patches-modules/src/poly_to_mono.rs:92-104` — sums 4 × 0.25 in the test,
  asserts module's sum == 1.0.
- `patches-modules/src/sah.rs:92-99` — asserts default-zero field reads
  back as zero before any trigger.

## Acceptance criteria

- [ ] Three tests removed
- [ ] `just inner -p patches-modules` green
- [ ] If a real invariant existed (e.g. broadcast vs per-channel write,
      inactive-voice exclusion), keep one focused test that varies inputs
      across channels rather than reads back a single value

## Notes

If `poly_to_mono` already has an "inactive voices excluded" test elsewhere,
deleting the sum-readback is safe; otherwise add a 2-channel test where
one voice is inactive and verify only the active value contributes.
