---
id: "0663"
title: Add Module::WANTS_PERIODIC const and default periodic_update
priority: high
created: 2026-04-24
epic: E114
adr: 0052
---

## Summary

Add `const WANTS_PERIODIC: bool = false` and
`fn periodic_update(&mut self, _pool: &CablePool<'_>) {}` to the
`Module` trait. Keep `PeriodicUpdate` and `as_periodic` in place for
now — this ticket only introduces the new surface so downstream tickets
can migrate incrementally.

## Acceptance criteria

- [ ] `Module::WANTS_PERIODIC: bool` associated const added with
      default `false`.
- [ ] `Module::periodic_update(&mut self, &CablePool<'_>)` default
      method added with empty body.
- [ ] Doc comment on `WANTS_PERIODIC` notes: evaluated at plan-build
      time, not per-tick; cannot depend on runtime state.
- [ ] Doc comment on `periodic_update` notes: invoked every
      `periodic_update_interval` samples when `WANTS_PERIODIC == true`.
- [ ] `cargo build -p patches-core` clean.
- [ ] Existing `as_periodic` / `PeriodicUpdate` usage untouched.

## Notes

Pure additive step so 0664/0665 can land in either order. No behaviour
change yet — nothing calls the new method.
