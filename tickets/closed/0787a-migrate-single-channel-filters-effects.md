---
id: "0787a"
title: Migrate single-channel filters and effects to TEMPLATE
priority: high
created: 2026-05-02
epic: "E132"
adrs: ["0066"]
depends_on: ["0786"]
---

## Summary

Migrate single-channel filter, drive, and effect modules in
`patches-modules` to declare a `const TEMPLATE` and delete their
`describe()` override.

In scope (representative): biquad/SVF filter modules, `drive`,
`limiter`, `compressor`, `pitch_shift` (single-channel), `hihat`
and other percussion synths, `reverb` family (mono), waveshapers.
Confirm final list against audit.

## Acceptance criteria

- [ ] Every in-scope module declares `const TEMPLATE`.
- [ ] Each module's `describe()` override removed.
- [ ] Descriptor output byte-identical pre/post migration.
- [ ] `cargo test -p patches-modules` passes.
- [ ] Integration tests touching effect modules pass unchanged.

## Notes

- Mechanical; ~12-15 modules.
- `pitch_shift` carries `length` / `high_quality` (post-E126:
  structural params) — those move within the param tables, not
  into/out of the template's structural axis.
