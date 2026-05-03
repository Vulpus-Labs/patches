---
id: "0785"
title: ModuleDescriptorTemplate types in patches-core
priority: high
created: 2026-05-02
epic: "E132"
adrs: ["0066"]
---

## Summary

Introduce `ModuleDescriptorTemplate` and supporting types
(`PortTemplate`, `ParameterTemplate`, `CountAxis`, `AxisId`) in
`patches-core`. Provide `build_channels(channels: u32) -> ModuleDescriptor`
that constructs an existing `ModuleDescriptor` from the template plus
a channel count. Internal multi-axis representation; single-axis
(`channels`) surface only.

No call sites change yet. Existing modules still use
`describe(shape)`. This ticket lands the type machinery only.

## Acceptance criteria

- [ ] `ModuleDescriptorTemplate` and related types added in
      `patches-core/src/modules/`.
- [ ] `build_channels(u32)` produces a `ModuleDescriptor` byte-equivalent
      to today's manual builder for a representative test case (write
      a unit test using a small synthetic template).
- [ ] Multi-axis `build(&[(AxisId, u32)])` exists but is exercised
      only by the channels-only convenience.
- [ ] Doc comments explain the distinction between global and
      per-axis ports/params.
- [ ] `cargo clippy` and `cargo test -p patches-core` clean.

## Notes

- Static (`&'static str` everywhere) — no allocation in the const.
- The owned/serde mirror lands in ticket 0790; this ticket is in-tree
  static types only.
- Future-proofing only: do not surface multi-axis to DSL or modules.
