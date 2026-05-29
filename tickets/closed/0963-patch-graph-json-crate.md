---
id: "0963"
title: patches-graph-json crate — schema, emitter, CLI; POC dump
priority: high
created: 2026-05-28
---

## Summary

Create a new `patches-graph-json` crate that serializes the expanded,
port-kind-resolved patch graph to JSON. The crate maps `patches_dsl::FlatPatch`
+ manifest-resolved port kinds + full provenance into serde "mirror" structs,
so `patches-dsl` itself gains no `serde` dependency (ADR 0079 §2). Ship a CLI
that emits JSON for a `.patches` file. Prove it against
`voice_template.patches`.

## Acceptance criteria

- [x] New workspace crate `patches-graph-json` with deps `patches-dsl`,
      `patches-manifest`, `serde`, `serde_json` (and `patches-core` for cable
      types). `publish = false`.
- [x] Mirror structs cover: modules (id, type_name, shape, resolved params,
      port aliases), connections (from/to module+port+index, cable `map`),
      per-port `CableKind`/`PolyLayout`, and **full** `Provenance` (expansion
      chains + source spans).
- [x] Port kinds resolved via `registry_from_manifest(bundled_manifest())` (the
      same lookup `patches-svg` uses); unknown module types degrade to
      unclassified, no error.
- [x] Pipeline mirrors `patches-svg`: `load_with` → `expand` → emit. Partial /
      invalid patches still emit (no interpreter pass).
- [x] CLI: emit JSON for a given `.patches` file to stdout (and/or `-o file`),
      with include-path resolution. Parse/expand errors surface as diagnostics,
      not panics.
- [x] `patches-dsl/Cargo.toml` unchanged (no `serde`).
- [x] POC: run the CLI on `patches-dsl/tests/fixtures/voice_template.patches`
      (the fixture used by `tests/expand/templates.rs`), eyeball the output,
      commit the artifact for review.
- [x] `just commit -p patches-graph-json` green; `cargo clippy` clean.

## Notes

- ADR 0079 (this is Phase 1), Epic E157.
- Schema is an external contract: keep it decoupled from `FlatPatch` internals
  so both can version independently. Consider a top-level `version` field now
  (cheap) even though the policy is deferred (ADR 0079 Open question 3).
- Layout (node positions) is deliberately **out** — presentation concern.
- The richer fully-validated `ModuleGraph` tier is out of scope (ADR 0079 Open
  question 2).
- Blocks 0964 (golden harness consumes this emitter) and 0966 (LSP request).
