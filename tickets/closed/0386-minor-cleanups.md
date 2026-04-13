---
id: "0386"
title: "Cleanup: swing docs, legacy alias, minor cosmetics"
priority: low
created: 2026-04-13
---

## Summary

Miscellaneous small cleanups identified during the post-v0.6.0 code review.

## Acceptance criteria

- [ ] Remove `pub type ExecutionState = ReadyState` legacy alias in `patches-engine/src/execution_state.rs` line 226; update any remaining references
- [ ] Fix unnecessary `let mut` in `patches-modules/src/tempo_sync.rs` line 128
- [ ] Normalise deref style in `patches-modules/src/drive.rs` `update_validated_parameters` (lines 149–160) — use consistent `*v` or auto-deref throughout
- [ ] `cargo clippy` and `cargo test` pass with no new warnings

## Notes

These are cosmetic issues with no functional impact. Bundle them into a single
commit.
