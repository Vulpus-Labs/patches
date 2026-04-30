---
id: "0766"
title: Cable range endpoint unit-family check and pitch unification
priority: medium
created: 2026-04-30
epics: ["E128"]
adrs: ["0062"]
---

## Summary

Resolve `uni`/`bi` endpoints to numeric values with unit-family
checking. Within the **pitch family**, note literals and Hz literals
both lower to v/oct (`C0 = 0`, `C1 = 1`, …; Hz → `log2(hz / hz_C0)`).
Cross-family pairs (e.g. `bi(440Hz, -12dB)`) are rejected with a
clear diagnostic naming both families.

Stops short of building the runtime triple — endpoints are evaluated
to `(f64, f64)` and held on a temporary AST/expand-side carrier.
Composition still falls back to the "not yet implemented" error
introduced in 0765.

## Acceptance criteria

- [x] Unit family detected per endpoint:
      `Pitch` (note_lit, Hz unit), `Time` (s/ms unit), `Level` (dB),
      `Plain` (no unit / numeric / `<param>`-resolved numeric),
      `Frequency` non-pitch — fold into `Pitch` since the only
      frequency unit is Hz.
- [x] Mixed Pitch endpoints lower to v/oct uniformly (notes already
      v/oct via `parse_note_voct`; Hz → v/oct over C0).
- [x] Cross-family endpoints error: `bi(440Hz, -12dB)` →
      `Code::InvalidCableScale` with message naming both families.
- [x] `Plain` is compatible with any other family (acts as raw
      multiplier; consumers decide interpretation).
- [x] `<param>`-resolved endpoints inherit the family of the resolved
      scalar; if unresolved (still a `ParamRef` after env lookup),
      treat as `Plain` for compatibility purposes — recompute happens
      at runtime in 0769.
- [x] Tests: pitch unification, cross-family rejection, plain+pitch
      mix, unresolved param ref behaves as plain.
- [x] `cargo test -p patches-dsl` and `cargo clippy` pass.

## Notes

Reference: [ADR 0062](../../adr/0062-cable-range-expressions.md).
Hz → v/oct constant: `C0 = 16.3516 Hz` (matches existing
`parse_note_voct`). Centralise the conversion alongside it.
