---
id: E157
title: Patch-graph JSON surface + golden test rewrite
status: open
created: 2026-05-28
---

## Goal

Emit the expanded, port-kind-resolved patch graph as a JSON document, and use
it to replace brittle hand-written expander/inliner slice-asserts with fixture
goldens. Design and rationale in **ADR 0079**. This epic is Phases 1 and 1.1;
it delivers standalone value (a diffable debug/interchange surface + better
test coverage) independent of the diagram-tooling work in **E158**.

Key decisions (ADR 0079):

- **Cheap tier:** expanded `FlatPatch` + per-port `CableKind`/`PolyLayout` from
  the bundled manifest + full provenance. Deps: `patches-dsl` +
  `patches-manifest` only — no interpreter, no `rust-sugiyama`. Partial/invalid
  patches still emit. Layout is **not** in the JSON (presentation concern).
- **Serde via mirror structs** in a new `patches-graph-json` crate;
  `patches-dsl` stays serde-free so the external schema and internal
  `FlatPatch` evolve independently.
- **Full provenance in output, redact in golden.** Spans are load-bearing for
  debug/diagram (hover, click-to-source); the golden harness redacts spans to a
  stable placeholder, canonicalizes module/connection ordering, and compares
  floats with epsilon.

## Scope

**In:**

- `patches-graph-json` crate: serde schema (mirror structs), emitter from
  `FlatPatch` + manifest-resolved port kinds + full provenance, and a CLI
  emitting JSON for a `.patches` file (stdout/disk, include-path resolution).
- POC: emit and review `voice_template.patches`; commit the artifact.
- Canonicalizing golden harness: span redaction (placeholder, not drop),
  module/connection ordering canonicalization, tolerant float compare.
  `insta`-backed.
- Migrate the structural slice-asserts in `patches-dsl/tests/expand/*`
  (templates, arity, params, scale, namespacing, provenance structure) to
  fixture goldens.

**Out (deferred / other epics):**

- LSP `patches/graphJson`, JS diagram render, retiring `patches-svg` — **E158**
  (Phases 2, 3).
- Fully-validated `ModuleGraph` (SCC/fusion/buffer-layout) JSON tier — ADR 0079
  Open question 2.
- Compiled patch artifact for player/CLAP load — spike ticket 0969, ADR 0079
  Open question 1.
- AST/parse-level tests and error-path tests stay as targeted tests (not
  golden-migrated).

## Tickets

- [x] [0963 — `patches-graph-json` crate: schema, emitter, CLI; POC dump](../../tickets/closed/0963-patch-graph-json-crate.md)
- [ ] [0964 — Canonicalizing golden harness (span redaction, ordering, float epsilon)](../../tickets/open/0964-graph-json-golden-harness.md)
- [ ] [0965 — Migrate `patches-dsl` expand tests to fixture goldens](../../tickets/open/0965-migrate-expand-tests-to-goldens.md)

## Dependency order

```text
0963 (emitter + CLI) ──> 0964 (golden harness) ──> 0965 (test migration)
```

## Acceptance

- `patches-graph-json` emits a JSON doc for a `.patches` file containing
  modules, connections, resolved params, per-port cable kinds, and full
  provenance; unknown module types degrade to unclassified (no error);
  partial/invalid patches still emit.
- `patches-dsl` gains no `serde` dependency.
- The golden harness redacts spans to a stable placeholder, sorts
  modules/connections canonically, and compares floats within `1e-12`; a
  fixture edit that shifts spans but not structure produces **no** golden diff.
- The structural tests in `tests/expand/templates.rs` and `tests/expand/arity.rs`
  that assert on FlatPatch slices are replaced by fixture goldens; AST tests,
  error-path tests, and a handful of negative-intent asserts remain.
- `just commit` green for touched crates; `cargo clippy` clean.

## Open questions

1. **CLI shape.** Standalone `patches-graph-json` bin vs a `--json` subcommand
   on an existing tool. Resolve in 0963; standalone bin is the default
   assumption.
2. **Golden granularity.** One golden per fixture vs per-aspect. Default: one
   per fixture (captures all slices); revisit if any golden becomes unwieldy.
