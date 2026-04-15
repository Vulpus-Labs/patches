---
id: "E082"
title: Pipeline consistency, diagnostics consolidation, DSL cleanup
created: 2026-04-15
status: open
depends_on: ["E079", "E080", "E081", "ADR-0038"]
tickets: ["0439", "0440", "0441", "0442", "0443", "0444", "0445", "0446", "0447", "0448"]
---

## Summary

Post-review cleanup of the work landed in E075/E076/E078/E079/E080/E081.
The staged pipeline, structured diagnostics, and expansion-aware LSP all
work, but a cross-crate review turned up three classes of issue:

1. **Consumer drift.** Player, CLAP, and LSP each reimplement error →
   diagnostic conversion; `pipeline_layering_warnings()` is emitted only
   by LSP; `run_all` / `run_accumulate` are bypassed by player and CLAP
   who still call `bind_with_base_dir()` directly. The pipeline is a
   design, not yet an enforced contract.
2. **Stringly-typed seams.** `classify_param_error` pattern-matches on
   message substrings; load codes are hard-coded strings; stage
   boundaries between `ParsedFile` / `ExpandedFile` / `BoundPatch` are
   not type-distinguished.
3. **Boilerplate.** `source_id_for_uri` copy-pasted across three LSP
   handlers; `expand_body` takes 9 parameters; `UnresolvedModule`
   construction repeated three times in `descriptor_bind`; structural
   code/label tables maintained in parallel.

Plus one latent bug: `Expander.alias_maps` is not scope-isolated between
template instantiations within a single `expand()` call.

This epic closes those gaps without introducing new user-facing
features. Work is parallelisable across crates; the only ordering
constraint is that 0439 (centralised converters) should land before
0440 (pipeline-level warning emission) so both can share the same
rendering path.

## Acceptance criteria

- [ ] Diagnostic rendering is single-sourced in `patches-diagnostics`;
      player, CLAP, and LSP consume it without reimplementation.
- [ ] `pipeline_layering_warnings()` is emitted inside the pipeline
      orchestrator, not per-consumer.
- [ ] `classify_param_error` is deleted; parameter errors are typed.
- [ ] `LoadErrorCode` enum mirrors `BindErrorCode`.
- [ ] `expand_body` and `emit_single_connection` take a context struct.
- [ ] Expander alias maps are scope-isolated per template instantiation.
- [ ] LSP handler plumbing shares one `with_expansion_context` helper
      and one `source_id_for_uri`.
- [ ] Stage boundaries carry newtype wrappers; `pipeline::parse()` is
      either deleted or becomes a real stage entry point.
- [ ] All four consumers (player, CLAP, LSP, WASM) produce identical
      diagnostics for identical source.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Tickets

| ID   | Title                                                          |
|------|----------------------------------------------------------------|
| 0439 | Centralise per-consumer diagnostic rendering                   |
| 0440 | Emit layering warnings from pipeline orchestrator              |
| 0441 | Typed parameter errors in descriptor_bind                      |
| 0442 | LSP handler boilerplate extraction                             |
| 0443 | ExpansionCtx struct for expand_body and emit_single_connection |
| 0444 | Expander alias-map scope isolation                             |
| 0445 | Deduplicate UnresolvedModule construction and require-resolved |
| 0446 | LoadErrorCode enum and typed load codes                        |
| 0447 | Stage-boundary newtypes and pipeline::parse cleanup            |
| 0448 | Consolidate StructuralCode code/label tables                   |

## Out of scope

- Parallel file loading (blocked on removing `SourceId` thread-local,
  separate ticket if pursued).
- New LSP features (code lens, rename, refactor).
- Incremental rebuild of `PatchReferences`.
