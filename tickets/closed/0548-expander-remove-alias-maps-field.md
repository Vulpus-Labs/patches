---
id: "0548"
title: Remove Expander::alias_maps global field (tier 3b of expander decomposition)
priority: medium
created: 2026-04-17
epic: E091
depends-on: "0547"
---

## Summary

Tier 3b of [ADR 0041](../../adr/0041-expander-decomposition.md)
removes the
`alias_maps: HashMap<String, HashMap<String, u32>>`
field from
[Expander](../../patches-dsl/src/expand/expander/mod.rs#L20-L29)
and threads the alias map as an explicit parameter through the pass
pipeline. After this ticket, `Expander` holds `{ templates, call_stack }`
— the two truly cross-frame pieces of state — and tier 4's
`BodyFrame` bundle becomes the obvious next step.

## Why now

`Expander::alias_maps` was always body-scoped in intent:

- populated during pass 1 of `expand_body` (both plain-module and
  template-instance branches),
- read during pass 2 by `expand_connection` /
  `emit_single_connection` to resolve alias-based port-index
  references,
- isolated per body by the tier-2 `AliasMapFrame` RAII guard that
  swaps a fresh map in on entry and restores the parent on exit.

The swap-and-restore machinery only exists because the data lives on
`Expander`. Moving the map to a local variable in `expand_body` and
threading it as a parameter makes scope isolation a property of the
stack frame — the same property every other body-local has — and
drops `AliasMapFrame` as a concept.

## Scope

### Type alias

Introduce in `expand/mod.rs` (or wherever the other shared types
live):

```rust
pub(in crate::expand) type AliasMap = HashMap<String, HashMap<String, u32>>;
```

A two-level map stays readable with the alias; promote to a newtype
only if a future caller proves the need.

### Files and changes

- [patches-dsl/src/expand/expander/mod.rs](../../patches-dsl/src/expand/expander/mod.rs)
  — drop the `alias_maps` field from `Expander`; delete
  `AliasMapFrame` and its `Drop` impl; `Expander::new` sets only
  `templates` and `call_stack`.

- [patches-dsl/src/expand/expander/passes.rs](../../patches-dsl/src/expand/expander/passes.rs)
  — `expand_body` constructs
  `let mut alias_map: AliasMap = HashMap::new();` at the top and
  threads `&mut alias_map` into `pass_modules` and `&alias_map`
  into `pass_connections`. `AliasMapFrame::push(self)` goes away;
  the fresh map at each body frame preserves scope isolation. The
  ticket-0444 comment block explaining the swap becomes a one-line
  note that the map is owned by the current body frame.

- [patches-dsl/src/expand/expander/template.rs](../../patches-dsl/src/expand/expander/template.rs)
  — `expand_template_instance` takes `&mut AliasMap` and does the
  `alias_map.insert(decl.name.name.clone(), instance_alias_map)`
  directly. The local lookup that reads back through
  `self.alias_maps.get(...)` collapses to using the freshly-built
  `instance_alias_map` value. `classify_call_args` (from ticket
  0547) already takes `alias_map: &HashMap<String, u32>` — its
  signature does not change.

- [patches-dsl/src/expand/expander/emit.rs](../../patches-dsl/src/expand/expander/emit.rs)
  — `expand_connection` and `emit_single_connection` gain
  `alias_map: &AliasMap` as an explicit parameter; internal
  lookups change from `self.alias_maps.get(...)` to
  `alias_map.get(...)`.

- Callers of `expand_connection` (currently only `pass_connections`)
  forward the alias map reference; callers of `pass_modules`
  (currently only `expand_body`) forward `&mut AliasMap`.

### Reading/writing discipline inside one body

Pass 1 (`pass_modules`) writes; pass 2 (`pass_connections`) reads.
Same order and same visibility as today — the only change is where
the map lives.

Inside `pass_modules`'s template-instance branch, `expand_template_instance`
takes `&mut AliasMap` and writes into it before recursing into
`self.expand_body(&template.body, ...)` for the child body. The child
body's `expand_body` constructs its own fresh local `AliasMap`, so
the child's writes do not pollute the parent's — isolation
preserved.

## Acceptance criteria

- [ ] `Expander` struct has exactly two fields: `templates` and
      `call_stack`.
- [ ] `AliasMapFrame` type and its `Drop` impl deleted.
- [ ] `grep 'self.alias_maps' patches-dsl/src/expand/` returns empty.
- [ ] `grep 'alias_maps:' patches-dsl/src/expand/` returns empty.
- [ ] `grep 'AliasMapFrame' patches-dsl/src/expand/` returns empty.
- [ ] `expand_connection` and `emit_single_connection` take
      `&AliasMap` as an explicit parameter.
- [ ] `expand_body` owns the `AliasMap` for its frame; the
      ticket-0444 comment block on `expand_body` is trimmed to
      reflect that scope isolation is now a stack-frame property.
- [ ] Public surface unchanged:
      `patches_dsl::expand::{expand, ExpandError, ExpandResult, Warning}`.
- [ ] `cargo build -p patches-dsl`, `cargo test -p patches-dsl`,
      `cargo clippy` clean workspace-wide.
- [ ] `expand_tests`, `torture_tests`, `structural_tests` pass with
      unchanged counts.

## Scope boundary

**In scope:** field removal, parameter threading through the passes
and the connection-emit path, `AliasMapFrame` removal, comment
trimming.

**Out of scope:**

- `BodyFrame` bundle (tier 4) — that ticket will fold the
  `AliasMap`, `BodyState`, and `ExpansionCtx` into one per-body
  frame and convert pass methods into free translator functions.
- Newtyping the two-level map (`AliasMap` stays a type alias).
- Changing any alias-lookup semantics, error codes, or messages.
- Reworking the recursion guard (`call_stack` / `CallGuard`) — it
  is genuinely cross-frame and stays on `Expander`.

## Notes

After this ticket, the only mutable state on `Expander` is
`call_stack`, which `CallGuard` already mediates. Tier 4 can then
consider whether `Expander` needs to remain a struct at all, or
whether the template table and call stack can be passed as two
parameters directly — but that is a tier-4 design question to
answer against the code as it stands *after* this tier lands.
