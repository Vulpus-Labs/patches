---
id: "0166"
title: "Expander: [*n] arity expansion, group param handling"
priority: high
created: 2026-03-20
epic: E028
depends_on: ["0165", "0162"]
---

## Summary

Update the template expander to handle arity expansion and group parameters.
After this ticket, `FlatPatch` remains fully concrete — no `PortIndex::Arity`
or group param entries survive expansion.

## Acceptance criteria

### Port index resolution

- [ ] `PortIndex::Literal(k)` → concrete index `k` (unchanged).
- [ ] `PortIndex::Param(name)` → look up `name` in `param_env`, coerce to
  `u32`. Error if absent or not a non-negative integer.
- [ ] `PortIndex::Arity(name)` → look up `name` in `param_env` to obtain N,
  then emit N `FlatConnection`s with indices `0..N`. Error if absent or not a
  non-negative integer. Both sides of a connection may independently carry
  `Arity`; if both do, their sizes must match or an error is returned.

### Template boundary rewiring with arity

- [ ] `$.in_port[*n]` in a template body registers N entries in `in_port_map`
  (`in_port/0` through `in_port/n-1`). A caller-side `instance.in_port[*n]`
  fans into all N entries.
- [ ] `$.out_port[*n]` registers N entries in `out_port_map` symmetrically.
- [ ] Scale composition applies independently to each expanded connection.

### Group param handling at call sites

- [ ] **Broadcast (scalar)**: `level: 0.8` at a call site for a group param
  `level[size]: float` distributes `0.8` to `level/0` through `level/size-1`
  in `FlatModule::params`.
- [ ] **Explicit array**: `level: [0.8, 0.9, 0.7, 1.0]` distributes elements
  by position. Error if array length ≠ N.
- [ ] **Per-index**: `level[0]: 0.8, level[1]: 0.3` sets those slots; slots
  not supplied use the declared default. Error if an index ≥ N is given.
- [ ] Group params with no call-site value use the declared default broadcast
  to all N slots.

### General

- [ ] `cargo clippy -p patches-dsl` passes with no warnings.
- [ ] `cargo test -p patches-dsl` passes.

## Notes

The "both sides carry `Arity`" check (`mixer.in[*size] <- $.in[*size]`): the
natural case is that both reference the same param, so their sizes trivially
agree. The expander should verify they agree after resolution rather than
assuming it.

Group param distribution produces indexed param entries (`level/0`, `level/1`,
…) in `FlatModule::params` using the existing `(name, Value)` pair with the
name formatted as `"level/0"` etc. — matching the existing `ParameterKey`
convention used in Stage 3 and `graph_yaml`.
