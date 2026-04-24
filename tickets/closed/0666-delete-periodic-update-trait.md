---
id: "0666"
title: Delete PeriodicUpdate trait and Module::as_periodic
priority: high
created: 2026-04-24
epic: E114
adr: 0052
depends_on: ["0664", "0665"]
---

## Summary

Final cleanup. Once the engine no longer calls `as_periodic` and no
module impls exist, delete `PeriodicUpdate` from `patches-core` and
remove `Module::as_periodic`. Clean up re-exports.

## Acceptance criteria

- [ ] `PeriodicUpdate` trait removed from
      `patches-core/src/modules/module.rs`.
- [ ] `Module::as_periodic` removed.
- [ ] `PeriodicUpdate` removed from `patches-core` public re-exports
      (`pub use` in `lib.rs`, `prelude`, etc.).
- [ ] No remaining references: `grep -rn PeriodicUpdate` across the
      workspace returns nothing outside ADR/epic/ticket/docs files.
- [ ] `cargo build --workspace` clean.
- [ ] `cargo test --workspace` green.
- [ ] v0.7.0 pre-release report updated: footgun #1 marked closed,
      line 108 "likely to change: `as_periodic` semantics" removed,
      line 202 "defer to v0.8" entry removed.

## Notes

Trivial once 0664 and 0665 land. Mostly a delete-and-recompile pass.
