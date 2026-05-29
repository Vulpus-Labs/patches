---
id: "0979"
title: Convert planner internal-invariant checks from panic to Result validation stages
priority: medium
created: 2026-05-29
---

## Summary

The planner's two internal invariants — `validate_fused_invariant` (fused cables
point forward in topo order) and `validate_scratch_fused_consistency` (a scratch
slot implies all consumers fused) — currently `assert!` / `panic!`. Make them
explicit **validation stages** that return `Result<(), PlanError>` with
structured variants, so deviant conditions are first-class, testable outcomes
asserted with a plain `Result` rather than `#[should_panic]`. This is the change
called out as especially valuable: clean testing of deviant conditions and happy
paths alike.

Scope is the planner's **internal** invariants only. User-facing validation
(unknown module, port / cable-kind mismatch) stays upstream in
`patches-interpreter`.

## Acceptance criteria

- [ ] Add structured `PlanError` variants for the invariants (e.g.
      `FusedOrderViolation { from, to, .. }`, `ScratchFusedConflict { producer,
      slot, consumer }`) carrying enough context to identify the offender.
- [ ] The two validators return `Result<(), PlanError>`; `make_decisions`
      propagates with `?`; the error maps through `BuildError` as today.
- [ ] Each validator has direct unit tests for **both** outcomes: a valid input
      returns `Ok`, and a hand-constructed deviant input returns the specific
      `Err` variant (assert the variant, not just "an error").
- [ ] Any retained `debug_assert` is a backstop for a believed-unreachable state
      and is documented as such — never the sole signal. Prefer removing it once
      the `Result` path is tested.
- [ ] The `#[should_panic]` validator tests are replaced by `Result`-asserting
      tests.
- [ ] `just push` green.

## Notes

Part of epic **E160** (ADR 0081), phase P3. Best done after 0977 so the
validators take typed IRs, but the conversion itself is nearly IR-independent and
may proceed on current shapes if 0977 slips (ADR 0081 / E160 open q.3). Keep the
plan-build invariant added for 0974 — this ticket upgrades it from panic to a
returned error and gives it an explicit happy-path test.
