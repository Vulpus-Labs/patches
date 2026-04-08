---
id: "0140"
title: Define `FlatPatch` IR in `patches-dsl`
priority: high
created: 2026-03-19
epic: E029
---

## Summary

Introduce the `FlatPatch` intermediate representation in `patches-dsl`. This
type is the output of the template expander (Stage 2) and the input to the
graph builder (Stage 3). It describes a patch as a flat list of concrete
module instances and connections — no template declarations, no template
instantiations, no `$`-prefixed port references.

Defining this type as its own ticket keeps the data model separate from the
algorithms that produce and consume it, and lets T-0141 and T-0143 start from
a stable interface.

## Acceptance criteria

- [x] `patches-dsl/src/flat.rs` (or similar) defines:
  - `FlatModule { id: String, type_name: String, shape: Vec<(String, Scalar)>, params: Vec<(String, Value)>, span: Span }`
  - `FlatConnection { from_module: String, from_port: String, from_index: u32, to_module: String, to_port: String, to_index: u32, scale: f64, span: Span }`
  - `FlatPatch { modules: Vec<FlatModule>, connections: Vec<FlatConnection> }`
- [x] All three types are `pub` and re-exported from the `patches-dsl` crate root.
- [x] All types derive `Debug` and `Clone`.
- [x] No semantic validation in the type definitions themselves (that belongs in T-0141 and T-0143).
- [x] `cargo clippy -p patches-dsl` passes with no warnings.

## Notes

`Scalar` and `Value` are imported from `patches-dsl::ast` — no new value types
are introduced here.

`from_index` / `to_index` default to `0` for unindexed port references during
expansion.

`scale` uses `f64` to match `Arrow::scale` in the AST; the graph builder
(T-0143) is responsible for range-checking and converting to `f32` when
calling `ModuleGraph::connect`.
