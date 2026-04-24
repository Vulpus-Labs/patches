---
id: "0687"
title: Module trait — default-method coverage via in-crate fake
priority: medium
created: 2026-04-24
epic: E117
---

## Summary

Surfaced by 0681: `patches-core/src/modules/module.rs` has 26/31 (84%)
survived mutants. Root cause is structural — the `Module` trait's
default method bodies (default `update_parameters`, `wants_periodic`,
`set_ports`, `as_tracker_data_receiver`, etc.) are not exercised by
any in-crate test, because concrete implementors live in
`patches-modules`.

## Acceptance criteria

- [ ] Add an in-crate minimal fake `Module` that relies on trait
      defaults (no overrides beyond the required methods).
- [ ] Exercise default behaviors: `update_parameters` validation path
      (including unknown-parameter rejection), `wants_periodic ==
      false`, `periodic_update` no-op, `as_tracker_data_receiver ==
      None`.
- [ ] Re-run mutants on `module.rs` — MISSED ratio should drop
      substantially.

## Notes

Keep the fake private to `patches-core` tests. Don't promote it to
`test_support/` (which is now excluded from mutation runs).
