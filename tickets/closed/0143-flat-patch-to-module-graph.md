---
id: "0143"
title: `FlatPatch`-to-`ModuleGraph` builder in `patches-interpreter`
priority: high
created: 2026-03-19
epic: E029
depends_on: ["0140", "0141"]
---

## Summary

Implement the graph builder (Stage 3) in `patches-interpreter`. It takes a
`FlatPatch`, a `patches_core::Registry`, and an `AudioEnvironment`, and
produces a `ModuleGraph`. This is where DSL-level validation happens: unknown
type names, unknown port names, and parameter type errors are all caught here
with source spans attached.

The existing `Registry` in `patches-core` (used by `Planner` and
`PatchBuilder`) is used directly — no new factory abstraction is needed.
`patches_modules::default_registry()` provides the built-in modules.

## Acceptance criteria

- [ ] `patches-interpreter` exports:
  ```rust
  pub fn build(
      flat: &FlatPatch,
      registry: &patches_core::Registry,
      env: &AudioEnvironment,
  ) -> Result<ModuleGraph, InterpretError>
  ```
- [ ] `InterpretError` is `pub`, carries a `Span` (from `patches-dsl::ast`) and
  a human-readable `message`, and is re-exported from the crate root.
- [ ] For each `FlatModule`:
  - Call `registry.describe(type_name, shape)` to get `ModuleDescriptor`; emit
    `InterpretError` (with `FlatModule::span`) if the type name is unknown.
  - Convert `params` (`Vec<(String, Value)>`) to a `ParameterMap`; emit an
    error for params whose name or value type is incompatible with the descriptor.
  - Call `ModuleGraph::add_module(id, descriptor, &params)`.
- [ ] For each `FlatConnection`:
  - Look up source and destination node descriptors (from the graph, via
    `graph.get_node()`) to validate port names and retrieve `PortDescriptor`s;
    emit `InterpretError` (with `FlatConnection::span`) for unknown port names.
  - Call `ModuleGraph::connect`; wrap any `GraphError` (kind mismatch,
    duplicate driver, out-of-range scale, etc.) in `InterpretError` with the
    connection's span.
- [ ] Connections are processed after all modules have been added, so
  forward references within the same patch are not an error.
- [ ] `cargo test -p patches-interpreter` and `cargo clippy -p patches-interpreter`
  pass with no warnings.

## Notes

`FlatConnection::scale` is `f64`; convert to `f32` before calling
`ModuleGraph::connect`. The range check (`[-1.0, 1.0]`) is enforced inside
`connect` — the builder does not need to duplicate it, but should wrap the
resulting `GraphError` with the span from `FlatConnection`.

`ModuleShape` is `patches_core::ModuleShape`. The DSL `shape` args
(`Vec<(String, Scalar)>`) need to be converted to a `ModuleShape` before
passing to `registry.describe()`. Check `ModuleShape`'s API for the correct
construction path.
