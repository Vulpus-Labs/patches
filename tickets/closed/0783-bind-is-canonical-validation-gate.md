---
id: "0783"
title: Assert bind is the canonical validation gate (no GraphError reaches users)
priority: low
created: 2026-05-02
---

## Summary

`patches-core::graphs::graph::GraphError` and
`patches-interpreter::descriptor_bind::BindError` independently encode
the same set of user-facing rules (cable kind, mono/poly layout,
duplicate input, unknown port, scale range). LSP runs `descriptor_bind`
only; the player/CLI run full `interpreter::build`, which also runs the
graph-stage checks. Historically a rule lived in graph but not bind, so
a bad patch compiled silently in the editor but failed at load (see
ticket 0782 for the audio→trigger case).

The principle going forward: **bind is the gate**. Every user-facing
rule must fire in bind. Graph-stage variants stay as defensive checks
but should be unreachable for any patch that passes bind.

## Acceptance criteria

- [ ] Regression test (in `patches-integration-tests` or
      `patches-interpreter`) that, for a curated corpus of bad patches
      covering each `GraphError` variant, asserts the failure surfaces
      as a `BindError` — never as an `InterpretError::ConnectFailed`
      wrapping a `GraphError`.
- [ ] Doc comment on `GraphError` flagging it as defensive: any variant
      reaching `interpreter::build` output indicates a missing bind
      check.
- [ ] Audit pass: confirm `DuplicateNodeId` / `NodeNotFound` are the
      only variants without a `BindErrorCode` mirror, and document why
      (internal builder state, not user-authored).

## Notes

- Variants currently mirrored: `OutputPortNotFound`/`InputPortNotFound`
  ↔ `UnknownPort`; `InputAlreadyConnected` ↔ `DuplicateInputConnection`;
  `ScaleOutOfRange` ↔ `ParameterConversion` (scale range); `Cable`/
  `Poly`/`MonoLayoutMismatch` ↔ same names in bind.
- Once the regression harness is in place, a future cleanup could lift
  the graph-stage checks into `debug_assert!` to make the
  defense-in-depth status explicit.
- Related: ticket 0782 closed by adding `MonoLayoutMismatch` to bind.
