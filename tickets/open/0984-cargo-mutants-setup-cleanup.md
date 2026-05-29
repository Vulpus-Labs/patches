---
id: "0984"
title: cargo-mutants setup with strict cleanup discipline
priority: medium
created: 2026-05-29
---

## Summary

Add `cargo-mutants` as a dev tool, configure it for `patches-planner`, and put
a cleanup contract in place so mutation runs never leave stale workspaces
behind. `cargo-mutants` writes worker directories and per-mutation logs under
`target/mutants.out/`; on a workspace this size a single run can leave several
GB of artefacts, and an interrupted parallel run can leave half-finished
workers. **The entry point must clean up on success, failure, and interrupt
without exception.**

## Acceptance criteria

- [ ] `cargo-mutants` installation documented (one-line `cargo install
      cargo-mutants` recorded in `MUTANTS.md` or `CLAUDE.md`).
- [ ] A `just mutants` recipe wrapping `cargo mutants -p patches-planner` with
      sensible defaults (timeout per mutation, `--jobs` count, mode chosen per
      the open question below).
- [ ] The recipe **always** removes `target/mutants.out/` after the run,
      including on:
  - [ ] normal success;
  - [ ] test failure or non-empty surviving-mutation list;
  - [ ] SIGINT / Ctrl-C interrupt;
  - [ ] per-mutation timeout or wall-clock cap.
  Implementation hint: a `bash` wrapper with `trap 'rm -rf
  target/mutants.out' EXIT INT TERM`, or a small Rust runner with `Drop`-based
  cleanup.
- [ ] `.gitignore` confirmed to cover `target/mutants.out/` (transitively via
      `target/`, but explicit if not).
- [ ] **Disk-usage measurement** recorded in `MUTANTS.md`: peak `target/`
      size during a full pass, steady-state size after cleanup. Steady-state
      must be ≤ baseline `target/` size before the run.
- [ ] **Worker-mode decision** recorded in `MUTANTS.md` (resolves E161 open
      question 1): `--in-place` vs default workers, with the measurement that
      justified the choice. If default-mode peak exceeds 10 GB, switch to
      `--in-place` and accept the lower parallelism.
- [ ] An advisory CI job (scheduled, e.g. nightly) runs `just mutants` and
      posts a one-line summary (mutations seen / caught / missed / timeout) as
      a repo comment or scheduled-job artefact. Non-blocking. Includes the
      same cleanup discipline.
- [ ] `MUTANTS.md` records: install command, run command, expected wall-clock,
      the cleanup contract, and the triage workflow for a surviving mutation.

## Notes

Part of epic **E161**, independent of 0981–0983 (can land in parallel).

The cleanup contract is the **load-bearing** acceptance criterion. A mutation
pass that leaves 30 GB of stale workers in `target/` is worse than no
mutation pass at all — it silently steals disk from local builds and from CI
runners, and the next run fails confusingly on out-of-space. Treat the trap /
defer / `Drop` cleanup as a correctness requirement, not as polish.

Worker-mode trade-off, restated for the implementation:

- **`--in-place`** (single tree, no copies): low disk, single-threaded by
  default. Failure mode: an interrupted run leaves the source tree
  patched with a mutation; the wrapper must `git checkout patches-planner/`
  on every exit path to restore it.
- **Default workers**: per-job copies in `target/mutants.out/worker-N/`,
  parallelisable, simpler interrupt semantics. Failure mode: high disk
  usage scales with `--jobs`.

Measure both modes on a 5-mutation dry run before settling. Document the
choice and the measurement.

The advisory CI job is intentionally **non-blocking**. Mutation testing is a
triage signal, not a gate: a regression in mutation coverage may be acceptable
if the surviving mutation is genuinely equivalent. Surface the result for
review, do not fail the build on it.
