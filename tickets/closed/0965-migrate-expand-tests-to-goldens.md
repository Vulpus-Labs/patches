---
id: "0965"
title: Migrate patches-dsl expand tests to fixture goldens
priority: medium
created: 2026-05-28
---

## Summary

Replace the hand-written FlatPatch slice-assertions in
`patches-dsl/tests/expand/*` with fixture goldens using the canonicalizing
harness (0964). Most of these tests load one fixture and assert on one slice of
the expansion (module set, a connection, a resolved param, a composed scale);
one golden per fixture captures every slice at once and catches regressions the
targeted asserts miss (ADR 0079 §4).

## Acceptance criteria

- [x] Structural slice-asserts in `tests/expand/templates.rs` and
      `tests/expand/arity.rs` (module namespacing, internal/boundary
      connections, scale composition, param substitution/defaults, shape args,
      group params, arity expansion counts, provenance *structure*) replaced by
      fixture goldens.
- [x] **Kept as targeted tests** (not migrated):
  - AST/parse-level tests (`ast_port_index_variants`, `ast_port_group_decl_arity`,
    `ast_param_decl_arity`, …) — they inspect the parsed `File` before
    expansion; wrong layer for a FlatPatch golden.
  - Error-path tests (`error_recursive_template`, `error_arity_*`) — substring
    match on the `Err` message; tolerant JSON equality doesn't apply.
  - A handful of **negative-intent** asserts (`v1`/`v2` must *not* be a
    FlatModule; provenance empty-chain and sibling-distinctness, both of which
    span redaction undoes) retained explicitly to document intent.
- [x] One golden per fixture (default); reuse existing fixtures
      (`voice_template.patches`, `nested_templates.patches`, `bus_size_3.patches`,
      `limited_mixer.patches`, …) where they already exist.
- [x] Net test count drops materially (templates.rs 14→5, arity.rs 16→7 = 30→12
      targeted; 11 fixture goldens added beside the existing `voice_template`)
      with no loss of coverage.
- [x] `just commit -p patches-dsl` green; `cargo clippy` clean.

## Outcome

Goldens live in `patches-graph-json/tests/golden.rs` (where the 0964 harness and
the `voice_template` proof golden already are), sharing the patches-dsl fixtures
via `include_str!`. New goldens: `nested_templates`, `bus_size_3`, `fan_size_4`,
`channel_indexed`, `scaled_fan_size_2`, `levelled_broadcast`,
`levelled_per_index`, `limited_mixer`, `zero_arity`, `bus_shape_arg`,
`provenance_two_level`. Each was eyeballed (connections, params, shape,
provenance chain length) against the asserts it replaced — not blind-accepted.

## Notes

- ADR 0079 §4 (Phase 1.1), Epic E157. Depends on 0964.
- Review discipline: goldens must be eyeballed on creation/update, not blindly
  `UPDATE=1`-accepted — the retained intent asserts are the guardrail.
- `support/mod.rs` helpers (`find_connection`, `assert_connection_scale`, etc.)
  stay for the retained targeted tests; don't delete wholesale.
