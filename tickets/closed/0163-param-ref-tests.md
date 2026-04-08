---
id: "0163"
title: "Tests: <param> syntax, unquoted strings, shorthand, structural interpolation"
priority: high
created: 2026-03-20
epic: E027
depends_on: ["0162"]
---

## Summary

Comprehensive tests for all new syntax introduced by E027. Existing tests
updated in earlier tickets cover regressions; this ticket adds positive-path
and error-path tests for the new forms specifically.

## Acceptance criteria

- [ ] **Unquoted string literals**: a module param `waveform: sine` round-trips
  through parser and expander to `Scalar::Str("sine")`; quoted `"sine"` gives
  the same result.
- [ ] **`<param>` in param blocks**: a template with `{ frequency: <freq> }`
  expands correctly when `freq` is supplied at the call site.
- [ ] **Shorthand param entry**: `{ <attack>, <decay>, release: 0.3 }` expands
  to the same `FlatModule::params` as `{ attack: <attack>, decay: <decay>,
  release: 0.3 }`.
- [ ] **Port label interpolation**: a template with `osc.<type>` in a
  connection, instantiated with `type: "out"`, produces a `FlatConnection`
  with `from_port: "out"`.
- [ ] **Scale interpolation**: `-[<gain>]->` with `gain: 0.5` produces
  `FlatConnection::scale == 0.5`. Scale composition at template boundaries
  works correctly when the inner or outer scale is a `ParamRef`.
- [ ] **Error: unknown ParamRef in port label**: a connection referencing
  `<nonexistent>` in port label position when no such param is in scope returns
  an `ExpandError` with a useful message.
- [ ] **Error: non-numeric ParamRef in scale**: `-[<waveform>]->` where
  `waveform` resolves to a string returns an `ExpandError`.
- [ ] `cargo clippy -p patches-dsl` and `cargo test -p patches-dsl` pass.
