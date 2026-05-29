---
id: "0964"
title: Canonicalizing golden harness for patch-graph JSON
priority: high
created: 2026-05-28
---

## Summary

Build the test harness that compares emitted patch-graph JSON (from 0963)
against committed goldens, with the canonicalization ADR 0079 §4 requires:
spans redacted to a stable placeholder, module/connection ordering
canonicalized, floats compared with epsilon. This is what lets goldens be
stable under fixture edits while still exercising the full expansion output.
`insta`-backed (already a `patches-svg` dev-dependency).

## Acceptance criteria

- [x] Span redaction: provenance source spans serialize to a stable placeholder
      (e.g. `"[span]"`) in the golden — **field retained, value redacted**, so
      presence/absence of provenance is still asserted. Expansion-chain
      *structure* (length, distinct call sites) is preserved, not redacted.
- [x] Ordering canonicalization before compare: modules sorted by id;
      connections sorted by `(from, from_port, from_index, to, to_port,
      to_index)`.
- [x] Tolerant float compare: scale/param floats compared within `1e-12`
      (round-before-serialize or numeric compare — pick one and document it).
- [x] `insta` redaction paths used for spans; ordering + float normalization
      applied in the serializer feeding the snapshot.
- [x] Proof: a golden for `voice_template.patches`. Editing the fixture in a way
      that shifts byte spans but not graph structure produces **no** golden diff
      (demonstrated in a test or documented manual check).
- [x] Helper API ergonomic enough that adding a fixture golden is: drop a
      `.patches` file, run with snapshot-update, eyeball, commit.
- [x] `just commit` green for touched crates; `cargo clippy` clean.

## Resolution

- `insta` (features `json` + `redactions`) added as a `patches-graph-json`
  dev-dependency.
- Float policy chosen: **round-before-serialize** to a `1e-12` quantum
  ([`golden::canonical_doc`]), so the stored golden is itself stable/diffable
  (documented in the module doc comment).
- Harness lives in `patches-graph-json/src/golden.rs` (`doc_from_src`,
  `canonical_doc`, `assert_graph_golden!`); ordering + float rounding in the
  serializer, span redaction via `insta` paths `.**.site` / `.**.expansion[]`.
- Proof golden committed: `tests/snapshots/golden__voice_template.snap`. The
  `byte_span_shift_produces_no_golden_diff` test demonstrates the no-diff
  invariant directly (prepend comment lines → spans move → canonical doc with
  spans flattened serializes identically).

## Notes

- ADR 0079 §4 (this is part of Phase 1.1), Epic E157.
- Depends on 0963 (the emitter). Blocks 0965 (test migration uses this harness).
- Where the harness lives: most natural in `patches-graph-json` (so the dsl
  tests depend on it) or as a small `dev-dependency`-only test-support module.
  Decide during implementation; keep `patches-dsl` serde-free regardless.
