---
id: "0374"
title: Fix dead drive_cv input in Drive module
priority: high
created: 2026-04-13
---

## Summary

The Drive module declares a `drive_cv` input in its descriptor
(`patches-modules/src/drive.rs` line 116) and reads it in `periodic_update`
(lines 220–235), but the value is discarded (`let _ = cv;`). The `process()`
method never reads the CV either. The input is advertised but non-functional.

## Acceptance criteria

- [ ] `drive_cv` modulates the drive amount in `process()` (additive or multiplicative, clamped to valid range)
- [ ] Test that non-zero CV alters the output
- [ ] Remove the dead `let _ = cv;` binding in `periodic_update` if CV is applied in `process()` instead

## Notes

Follow the pattern used by Bitcrusher's `rate_cv` / `depth_cv` for consistency.
