---
id: "0468"
title: IncludeGraph struct for workspace include coordination
priority: medium
created: 2026-04-15
---

## Summary

`patches-lsp/src/workspace/mod.rs:122-142` holds three sibling
HashMaps (`included_by`, `includes_of`, `artifacts`) that must
be kept in sync. The single `Mutex` covers ordering but the
*invariants between maps* live only in comments and code
discipline.

Invalidation walks ancestors via `includes_of`; if mutated
mid-traversal the walk is unsafe. `rewrite_include_edges`
(`workspace/mod.rs:314`) updates two maps with no enforced
coupling. The current code isn't buggy but the structure is
fragile.

## Acceptance criteria

- [ ] `IncludeGraph` struct wrapping `included_by` + `includes_of`
      with methods `add_edge`, `remove_edges_from(parent)`,
      `ancestors_of(uri)`, `rewrite_edges(parent, new_children)`.
- [ ] Invariants documented on the struct (e.g. "edge in
      `includes_of` iff reverse edge in `included_by`").
- [ ] Workspace holds one `IncludeGraph` + one `artifacts` map;
      no manual cross-map updates.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

E084. Pairs with 0459 (workspace split). Once state is its own
module, formalising the include graph is natural.
