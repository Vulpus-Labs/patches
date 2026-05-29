---
id: E161
title: Planner property-based + mutation testing
status: closed
created: 2026-05-29
closed: 2026-05-29
---

## Goal

Add a property-based test layer over the planner's allocation and topology
invariants, then audit the resulting suite with mutation testing. The
single-derivation-site fix (E160 / ADR 0081) made the 0974 class structurally
impossible by construction; property tests push that further by checking the
invariants hold over arbitrary graphs and replan histories, and mutation
testing measures whether the combined unit + property suite actually catches
deliberate planner regressions.

Two through-lines:

- **Invariants as universal statements.** Each load-bearing rule (slot
  uniqueness, fused ⇒ forward, scratch ⇒ all-consumers-fused, slice-position
  single source, cycle-slot stability) is expressed as a property that holds
  for every well-formed input — not just curated examples. proptest's shrinker
  gives minimal counter-examples on failure.
- **Cleanup is a hard requirement, not polish.** `cargo-mutants` writes
  worker directories and per-mutation logs under `target/mutants.out/`. On a
  workspace this size a naïve run leaves several GB of stale artefacts, and an
  interrupted parallel run can leave half-finished workers behind. Every
  mutation-testing entry point (Justfile recipe, CI job, wrapper script) must
  remove its temporary state on success, failure, and interrupt.

## Scope

**In:**

- proptest generators for module descriptors, `ModuleGraph`s, and replan edit
  histories.
- Single-build properties: slot uniqueness, fusion classification, scratch /
  fused consistency, slice-position single-source, region containment.
- History-replay properties: cycle stability across churn, mass conservation,
  tombstone correctness, `build_draft` determinism.
- `cargo-mutants` configuration, a `just mutants` recipe with a strict
  cleanup contract, and an advisory CI job.
- Triage of every surviving mutation: a covering test, or a `#[mutants::skip]`
  annotation with a one-line justification.

**Out (deferred / other work):**

- Audio-thread adoption ordering (engine crate, ADR 0051).
- Realtime safety (Loom / sanitisers).
- FFI-plugin `scratch_base_offset` edge cases beyond the existing targeted
  tests in `backplane_bind.rs`.
- Performance regressions — needs criterion benches, not PBT.

## Tickets

- [ ] [0981 — proptest scaffolding: generators + harness](../../tickets/open/0981-pbt-scaffolding.md)
- [ ] [0982 — Single-build invariants under proptest](../../tickets/open/0982-pbt-single-build-invariants.md)
- [ ] [0983 — History-replay invariants under proptest](../../tickets/open/0983-pbt-history-replay-invariants.md)
- [ ] [0984 — `cargo-mutants` setup + cleanup discipline](../../tickets/open/0984-cargo-mutants-setup-cleanup.md)
- [ ] [0985 — Mutation pass: `state/alloc` + `state/mod`](../../tickets/open/0985-mutation-pass-alloc-state.md)
- [ ] [0986 — Mutation pass: `builder` + `graph_index`](../../tickets/open/0986-mutation-pass-builder-graph-index.md)

## Dependency order

```text
0981 ─┬─> 0982 ─┐
      └─> 0983 ─┤
                ├─> 0985 ─> 0986
0984 ───────────┘
```

0984 is independent infrastructure and can land in parallel with 0981–0983.
0985 needs both a PBT-enriched suite (0982 at minimum) and the mutation
infrastructure (0984). 0986 follows 0983 and 0985.

## Acceptance

- All listed properties pass under proptest with default case count, in under
  30 s total runtime.
- `just mutants` (or equivalent) runs the configured pass and cleans up its
  workspace under all exit conditions (success, failure, Ctrl-C, timeout).
- After 0985 and 0986: every surviving mutation is either covered by a new
  test or annotated `#[mutants::skip]` plus a one-line justification.
- `just push` green throughout; `just smoke` green for tickets that touch
  integration tests.
- A `MUTANTS.md` (or a section in `CLAUDE.md`) records the install command,
  run command, expected wall-clock, and the cleanup contract.

## Open questions

1. **`cargo-mutants` worker mode.** `--in-place` avoids worker copies (low
   disk) at the cost of mutating the source tree, which breaks if interrupted.
   Workers (default) parallelise better but cost ~50–500 MB of `target/` per
   worker × jobs. Resolve in 0984 by measuring on this workspace and recording
   the choice.
2. **PBT crate.** proptest is the assumed choice (mainstream, strong shrinker).
   `quickcheck` is simpler but its shrinker is weaker; revisit only if
   proptest's macro overhead is noisy in this codebase.
3. **Mutation-survivor budget.** Some mutations are genuinely equivalent
   (e.g. permuting the iteration order of a collection that is later sorted).
   The triage convention — covering test vs `#[mutants::skip]` with reason —
   is settled as the first 0985 results come in.
