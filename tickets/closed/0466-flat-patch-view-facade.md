---
id: "0466"
title: FlatPatchView facade for LSP
priority: high
created: 2026-04-15
status: closed-wontfix
---

## Resolution

Closed without action. `FlatPatch` is consumed by signal_graph,
shape_render, peek, inlay, expansion, and hover — a trait facade
would either re-expose ~90% of the struct (ceremony without
unlocking polymorphism anyone needs) or hide details callers
genuinely need and force cumbersome recasts.

The honest framing is that `patches-dsl` is a shared crate whose
job is to expose the post-expansion artifact; `FlatPatch` *is*
the published contract. Field access by sibling crates is
legitimate. If layout-change friction becomes real later, mark
`FlatPatch` `#[non_exhaustive]` and version the changes — trait
indirection is not the cheapest mitigation.

## Summary

LSP imports `FlatPatch`, `FlatModule`, `FlatConnection`,
`SourceMap` directly from `patches-dsl` (e.g.
`patches-lsp/src/hover.rs:9-11`,
`patches-lsp/src/workspace/mod.rs:22`) and walks their fields.
These are post-expansion artifacts, not a published contract.
A FlatPatch layout change (e.g. splitting connections into
subcategories) silently breaks LSP.

`PatchReferences` already wraps FlatPatch as an index; that's
the natural boundary. Hover/inlay/peek shouldn't walk the raw
struct.

## Acceptance criteria

- [ ] Public `FlatPatchView` trait (or equivalent facade) in
      `patches-dsl` exposing the queries LSP actually needs.
- [ ] `FlatPatch` impls the trait; LSP imports the trait, not
      the struct fields.
- [ ] `PatchReferences` remains internal optimisation — public
      API to LSP is the trait.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

E084. The boundary already exists in spirit (`PatchReferences`);
this makes it real and enforced by the type system.
