---
id: "0167"
title: "Tests: variable arity expansion, group params, error cases"
priority: high
created: 2026-03-20
epic: E028
depends_on: ["0166"]
---

## Summary

Comprehensive tests for variable-arity template expansion introduced by E028.

## Acceptance criteria

- [ ] **`[*n]` expansion — basic**: a template with `in: in[size]` and a
  connection `mixer.in[*size] <- $.in[*size]`, instantiated with `size: 3`,
  produces exactly 3 `FlatConnection`s with indices 0, 1, 2.
- [ ] **`[k]` param index**: a connection `bus.in[channel]` with `channel: 2`
  produces a single `FlatConnection` with `to_index: 2`.
- [ ] **Template boundary with arity**: `$.in[*n]` rewiring produces N
  in-port map entries; caller-side connections fan into each correctly.
- [ ] **Scale composition with arity**: each of the N expanded connections
  carries the composed scale independently.
- [ ] **Group param — broadcast**: `level: 0.8` on a `level[size]: float`
  group with `size: 3` produces `level/0: 0.8`, `level/1: 0.8`, `level/2: 0.8`
  in `FlatModule::params`.
- [ ] **Group param — explicit array**: correct distribution; error on length
  mismatch.
- [ ] **Group param — per-index**: supplied indices set, others use default.
- [ ] **`LimitedMixer` example**: the full example from ADR 0019 expands
  correctly end-to-end (parse → expand → check flat patch structure).
- [ ] **Error: arity param missing**: `[*nonexistent]` returns `ExpandError`.
- [ ] **Error: arity mismatch**: mismatched `[*n]` on both sides of a
  connection (different resolved values) returns `ExpandError`.
- [ ] **Error: array length mismatch**: explicit array with wrong length
  returns `ExpandError`.
- [ ] `cargo clippy -p patches-dsl` and `cargo test -p patches-dsl` pass.
