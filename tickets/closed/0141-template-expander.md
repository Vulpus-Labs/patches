---
id: "0141"
title: Template expander in `patches-dsl`
priority: high
created: 2026-03-19
epic: E029
depends_on: ["0140"]
---

## Summary

Implement the template expander (Stage 2) in `patches-dsl`. It takes a parsed
`File` AST and returns a `FlatPatch` with all templates inlined, all
parameters substituted, and all cable scales composed at template boundaries.
The output contains only concrete module IDs and concrete port-to-port
connections — no template declarations or `$`-prefixed references remain.

## Acceptance criteria

- [x] `patches-dsl` exports `pub fn expand(file: &File) -> Result<FlatPatch, ExpandError>`.
- [x] `ExpandError` carries a `Span` and a human-readable `message`; it is
  `pub` and re-exported from the crate root.
- [x] Module instances from the top-level patch get their AST `name` as the
  node ID verbatim.
- [x] Module instances from a template body get node IDs namespaced as
  `<instance_name>/<inner_name>` (e.g., template `voice` instantiated as `v1`
  with inner module `osc` → node ID `v1/osc`). Nested templates extend the
  path: `v1/sub/osc`.
- [x] Template parameters (`Scalar::Ident` values) are substituted with the
  caller-supplied concrete values (or the parameter's declared default if
  omitted) before appearing in `FlatModule::params`.
- [x] Error if a required template parameter (no default) is not supplied at
  the call site.
- [x] Error if an unknown parameter name is supplied at a call site.
- [x] Template in-ports (`$.in_port` on the RHS of a connection inside the
  template body) are resolved: each caller-side connection driving
  `<instance>.in_port` is rewired directly to the inner module port, with
  scales multiplied along the path.
- [x] Template out-ports (`$.out_port` on the LHS inside the template body)
  are resolved symmetrically.
- [x] A template in-port may fan out to multiple inner ports (multiple
  connections with `$.x` on the RHS); each becomes a separate `FlatConnection`.
- [x] Scale composition: if the caller connects with scale `a` and the
  template body wires the boundary port with scale `b`, the resulting
  `FlatConnection::scale` is `a * b`.
- [x] Nested template instantiation (a template body that instantiates another
  template) is handled recursively.
- [x] Error on self-referential or mutually-recursive template instantiation
  (cycle detection).
- [x] All existing `patches-dsl` tests continue to pass.
- [x] Unit tests cover: flat patch passthrough, single template expansion,
  nested expansion, parameter default usage, missing required param error,
  recursive template error.
- [x] `cargo test -p patches-dsl` and `cargo clippy -p patches-dsl` pass with
  no warnings.

## Notes

The expander is purely structural — it does not know about module types, port
descriptors, or cable kinds. Port name validation happens in Stage 3 (T-0143).

`Connection::arrow.direction` (Forward / Backward) should be normalised during
expansion: a backward arrow `a.x <- b.y` is stored as `FlatConnection { from:
b.y, to: a.x }` with the same scale semantics as a forward arrow.
