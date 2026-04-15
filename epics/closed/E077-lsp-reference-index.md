---
id: "E077"
title: LSP patch reference index
created: 2026-04-15
depends_on: ["E075"]
tickets: ["0420", "0421"]
---

## Summary

Ticket 0419 landed expansion-aware hover on top of a flattened `FlatPatch`
and a forward span index. Over the course of fan-out handling and
template wiring, the hover module grew several ad-hoc traversals: inverse
lookups from call-site span → emitted modules, connection-span → fan-out
group, and template-body walks to recover `$.port ↔ inner.port` wiring
for each template call. See ADR 0037 for the full rationale.

This epic lifts those traversals into a single per-root `PatchReferences`
index built alongside `FlatPatch` in `ensure_flat_locked`, with the same
invalidation lifetime, and migrates the hover handlers to use it.

Out of scope:

- Inlay hints, peek expansion, and signal-graph diagnostics are the
  callers that motivate the index; their handlers are not written here.
  Building the index without those consumers risks speculating on their
  needs — they arrive in follow-up epics and reuse `PatchReferences`
  tables as-is or extend them.
- Cross-crate exposure: `PatchReferences` stays LSP-private. If `patches-svg`
  or another consumer wants the same structure it can be lifted out later.

## Acceptance criteria

- [ ] `patches-lsp::expansion::PatchReferences` exists with the fields
      described in ADR 0037, built from `(FlatPatch, &File)`.
- [ ] `ensure_flat_locked` caches `PatchReferences` in place of `SpanIndex`
      and invalidates it together with `flat_cache`.
- [ ] Hover handlers (call-site, connection-span, template-wiring,
      definition-site) consume `PatchReferences` tables; no hover code
      path walks `flat.modules` / `flat.connections` / `merged.templates`
      directly.
- [ ] Ad-hoc helpers removed from `hover.rs`: `find_template_for_call_site`,
      `find_module_decl_type_name`, `collect_port_wires`,
      `hover_for_connection_group`'s internal filter, and the call-site
      smallest-enclosing scan.
- [ ] Existing hover tests pass unchanged; new tests cover the index
      builder directly (call-site grouping, fan-out grouping, template
      wiring table).
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Tickets

| ID   | Title                                             |
|------|---------------------------------------------------|
| 0420 | Introduce PatchReferences index and builder       |
| 0421 | Migrate expansion-aware hover to PatchReferences  |
