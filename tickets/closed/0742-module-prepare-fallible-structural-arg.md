---
id: "0742"
title: Reshape Module::prepare to fallible and absorb StructuralParams
priority: high
created: 2026-04-28
epic: "E126"
parent: "0734"
adrs: ["0060"]
---

## Summary

Second slice of 0734 (ADR 0060). Change `Module::prepare` to take
`&StructuralParams` and return `Result<Self, BuildError>`. Update
every module impl, FFI plugin, test plugin, and the test harness
together so the workspace stays compiling. Modules ignore the
`StructuralParams` arg (empty in practice — no module declares
structural params yet). Behaviour unchanged.

## Acceptance criteria

- [ ] `Module::prepare` signature:
      `fn prepare(&AudioEnvironment, ModuleDescriptor, InstanceId, &StructuralParams) -> Result<Self, BuildError>`.
- [ ] Every concrete `Module` impl updated. Bodies wrap existing
      construction in `Ok(...)`.
- [ ] `Module::build` default impl threads an empty
      `StructuralParams` through `prepare` and propagates the result.
- [ ] `ModuleHarness::build_full` / `build_with_shape` /
      `build_with_env` accept an optional `StructuralParams`
      (default empty) and propagate prepare errors.
- [ ] FFI `prepare` ABI entry signature updated to carry an opaque
      structural blob (zero-length for now). Decode helpers unchanged
      until 0739.
- [ ] All test plugins updated.
- [ ] `cargo test` and `cargo clippy` pass on inner-loop and full
      workspace.

## Notes

No `update_parameters` changes; that lands in 0743. No module
behaviour changes. The structural arg threads through but is unused.

Mechanical breakage: ~50 module impls in `patches-modules`, FFI/wasm
plugins, test-plugins, and test-only modules. Plan a single
sweep-PR; partial state will not compile.
