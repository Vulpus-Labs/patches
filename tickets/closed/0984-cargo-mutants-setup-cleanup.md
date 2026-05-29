---
id: "0984"
title: cargo-mutants setup with strict cleanup discipline
priority: medium
created: 2026-05-29
closed: 2026-05-29
---

## Done

- [tools/run-mutants.sh](../../tools/run-mutants.sh) — wrapper installing
  `trap cleanup EXIT INT TERM` (separate handlers; the INT/TERM
  handlers `trap - EXIT` before re-running cleanup to avoid double
  cleanup, then exit 130 / 143). Removes `mutants.out/`,
  `mutants.out.old/`, and best-effort sweeps stale
  `$TMPDIR/cargo-mutants-*.tmp` worker dirs younger than 4 hours.
  Avoids `exec cargo mutants` (would replace the shell and skip the trap).
- `just mutants` — Justfile recipe forwarding extra args to the wrapper.
- [.github/workflows/mutants.yml](../../.github/workflows/mutants.yml) —
  advisory nightly job (07:00 UTC) + `workflow_dispatch`. Installs
  `cargo-mutants ^27 --locked`, runs `just mutants --keep-output`,
  uploads `mutants.out/` as a 14-day artefact, `continue-on-error: true`
  so missed mutations never fail the workflow. Summary step prints a
  per-outcome tally from `outcomes.json`.
- [MUTANTS.md](../../MUTANTS.md) — install command, run examples,
  wall-clock estimate, cleanup contract, worker-mode decision, triage
  workflow.
- `.gitignore` already covers `mutants.out/` (existing
  `mutants.out/` / `mutants.out.old/` entries).

## Worker-mode decision (E161 open question 1)

Chose **default worker mode with `--jobs 4`** over `--in-place`.

Measurement on this workspace (`--shard 0/30` → 7 mutations):

| Mode               | wall-clock | disk footprint                       |
| ------------------ | ---------- | ------------------------------------ |
| `--in-place` j=1   | 10 s       | 216 KB (mutants.out only)            |
| default workers j=2| 23 s       | 360 KB mutants.out; transient TMPDIR |

Both modes left `target/` flat (10 GB baseline, unchanged after run).
`--in-place` is faster for tiny shards (no worker tree copy) but
modifies `patches-planner/src/` directly — a hard interrupt during
mutation requires `git checkout` to recover. Worker mode writes to
`$TMPDIR/cargo-mutants-*.tmp` per mutation, cleaned per-mutation by
cargo-mutants; the working tree is never touched.

Worker mode chosen for **safer interrupt semantics** and for the
parallelism win on a full 181-mutation pass (`--jobs 4` amortises the
worker-copy setup across all mutations).

## End-to-end verification

```sh
$ just mutants --shard 0/30
7 mutants tested in 24s: 1 missed, 2 caught, 4 unviable
[run-mutants] removing mutants.out/ (exit 2)

$ just mutants --shard 0/30 --keep-output
7 mutants tested in 28s: 1 missed, 2 caught, 4 unviable
[run-mutants] --keep-output: retaining mutants.out/ for triage (exit 2)
```

Cleanup fires on non-zero exit. `--keep-output` retains for triage.
Exit code 2 (mutations missed) is forwarded.

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
