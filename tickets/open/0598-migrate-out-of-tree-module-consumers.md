---
id: "0598"
title: Migrate out-of-tree module consumers to `ParamView`
priority: high
created: 2026-04-20
depends_on: ["0596"]
---

## Summary

Migrate every remaining `update_validated_parameters` impl
outside `patches-modules`:

- [patches-vintage/src/vchorus/core.rs](../../patches-vintage/src/vchorus/core.rs)
  and any sibling vintage modules.
- [patches-profiling/src/timing_shim.rs](../../patches-profiling/src/timing_shim.rs)
  — the passthrough shim used in profiling harnesses.
- [patches-registry/src/registry.rs](../../patches-registry/src/registry.rs)
  — test-only stub modules.

`patches-wasm` is unmaintained and explicitly excluded; do not
migrate it. If the trait flip breaks its build, leave it broken
— that crate is not part of the workspace test gate.

## Scope

Same mechanical shape as 0597: `&ParameterMap` → `&ParamView<'_>`,
`params.get_scalar` → typed accessor.

## Acceptance criteria

- [ ] All listed crates build and test green.
- [ ] Workspace `cargo test`, `cargo clippy` clean.
- [ ] No `ParameterMap` references remain on any module's
      update path across the workspace.

## Non-goals

- WASM ABI changes.
- FFI ABI changes (spike 7).
- Shadow oracle retirement (0600).
