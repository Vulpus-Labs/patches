---
id: E160
title: Planner pipeline restructure — typed analysis/validation/transformation stages
status: open
created: 2026-05-29
---

## Goal

Restructure `patches-planner` so plan-building is an explicit, typed pipeline of
stages — analysis, validation, transformation — each a function
`fn(IR_prev) -> Result<IR_next, PlanError>`, every stage independently and
extensively unit-tested. Design and rationale in **ADR 0081**.

Two through-lines from the ADR:

- **Result-based error signalling.** Internal-invariant violations return a
  structured `PlanError` instead of panicking, so deviant conditions are
  first-class testable outcomes (no `#[should_panic]`), alongside happy paths.
- **No parameter churn.** A bundle of fields, once finalised by a stage, is
  passed forward **frozen** and read from that single source by later stages —
  never re-mapped into fresh fields or re-derived. Exception: discard a bundle /
  narrow the interface when later stages no longer need it, rather than carry
  dead data. (This is the structural fix for the 0974 class, where the same fact
  was re-derived at three sites and diverged.)

The work is staged and **behaviour-preserving**: audio goldens and the
integration suite must stay green/bit-identical throughout. This is a structure
and testability change, not a behaviour change.

## Scope

**In:**

- Lock-in unit tests at today's stage boundaries (before any refactor):
  `allocate_buffers` cable-buffer logic and the action-phase output, plus
  `make_decisions` orchestration.
- Typed IR bundles (`Topology`, `PortClassification`, `BufferLayout`,
  `PlanDraft`) composed from frozen prior bundles; `make_decisions` rewired to
  thread them; per-stage `IR_prev → IR_next` tests.
- Action-phase split: pure transforms behind a thin impure shell
  (`registry.create` / `set_ports` / id minting only); injected `InstanceId`
  source for deterministic tests.
- Panic → `Result` conversion of the planner's internal invariants, with
  structured `PlanError` variants and deviant-condition tests.
- Coverage fill: `Planner` / `build_full` replan + tracker-receiver index
  threading, multi-replan stress, poly/stereo input-resolve variants.

**Out (deferred / other work):**

- User-facing validation (unknown module, port / cable-kind mismatch) — stays in
  `patches-interpreter` upstream; the planner does not re-validate user input.
- Parallel execution / SCC thread partitioning (ADR 0072 desiderata) — the IR
  split should keep this a contained future change but does not implement it.
- Any change to `ExecutionPlan` semantics consumed by the engine — the plan's
  observable content is unchanged.

## Tickets

- [ ] [0975 — Lock-in unit tests for `allocate_buffers` cable-buffer logic](../../tickets/open/0975-allocate-buffers-cable-tests.md)
- [ ] [0976 — Lock-in tests: action-phase ExecutionPlan output + `make_decisions` orchestration](../../tickets/open/0976-action-phase-make-decisions-tests.md)
- [ ] [0977 — Typed IR bundles; rewire `make_decisions` to thread them (no logic change)](../../tickets/open/0977-typed-ir-bundles.md)
- [ ] [0978 — Split action phase: pure transforms + thin impure shell; inject InstanceId](../../tickets/open/0978-action-phase-pure-split.md)
- [ ] [0979 — Panic → Result internal-invariant validation stages](../../tickets/open/0979-result-based-validation.md)
- [ ] [0980 — Coverage fill: Planner replan/state threading, tracker indices, resolve variants](../../tickets/open/0980-planner-coverage-fill.md)

## Dependency order

```text
0975 ┐
     ├─> 0977 (IR) ─┬─> 0978 (action split) ─┐
0976 ┘              └─> 0979 (Result validation) ─┴─> 0980 (coverage fill)
```

0975 and 0976 are independent lock-in work and can run in parallel / first. 0977
threads the typed IRs. 0978 and 0979 build on 0977 and are independent of each
other. 0980 closes remaining holes last.

## Acceptance

- Plan-building reads as a sequence of typed stages, each
  `fn(IR_prev) -> Result<IR_next, PlanError>`; each stage has direct unit tests
  covering happy path, edge cases, and (for fallible stages) error conditions.
- The planner's internal invariants return `PlanError` (asserted in tests with
  plain `Result`); any remaining `debug_assert` is a backstop, never the only
  signal.
- No stage re-derives a fact another stage already finalised; facts are computed
  once and read from their owning bundle. A grep audit shows the slice-position /
  fused / port-classification facts each have a single derivation site.
- The action-phase pure transforms are unit-tested **without** a registry; the
  impure shell is limited to instantiation/effects and takes an injected
  `InstanceId` source.
- Audio goldens for example/fixture patches are bit-identical before and after;
  `just push` green; `just smoke` green (integration tests touched).

## Open questions

1. **IR ownership.** Borrowed (`<'a>`) vs owned `PlanDraft` — resolve in 0977/0978
   (ADR 0081 open q.1).
2. **`PlanError` granularity.** One `Internal { context }` vs distinct variants
   per invariant — favour distinct where a test asserts a specific condition
   (ADR 0081 open q.2).
3. **Whether 0979 can land before 0977.** Converting the two existing validators
   to `Result` is nearly IR-independent; if 0977 slips, 0979 may proceed first on
   the current shapes. Default order keeps it after 0977 so validators take typed
   IRs.
