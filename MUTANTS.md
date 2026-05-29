# Mutation testing — `cargo-mutants` on `patches-planner`

Mutation testing for the planner crate, scaffolded under epic **E161**.
See [epics/open/E161-planner-property-mutation-testing.md](epics/open/E161-planner-property-mutation-testing.md)
for the wider context (PBT + mutation pair) and ticket
[0984](tickets/closed/0984-cargo-mutants-setup-cleanup.md) for the
infrastructure rationale.

## Install

```sh
cargo install cargo-mutants
```

Version pinned in tests so far: **27.0.0**. Bumping cargo-mutants may
shift mutation generation; re-run `just mutants` and update the triage
notes (per 0985 / 0986) on a version bump.

## Run

```sh
just mutants                       # full pass on patches-planner
just mutants --shard 0/4           # shard 1 of 4 (parallel laptops / CI)
just mutants --file 'state/*.rs'   # subset by source file
just mutants --keep-output         # retain mutants.out/ for triage
just mutants -- --test '*'         # forward to cargo mutants: all tests
```

Anything you pass to `just mutants` (except `--keep-output`) is
forwarded to `cargo mutants` ahead of the fixed `-- --test properties`
test selector.

## Wall-clock

- Sample (7 mutations via `--shard 0/30`): ~10 s in-place, ~23 s
  workers (jobs=2).
- Full pass on a clean `target/`: estimated **~3 min** with the default
  `--jobs 4`. Re-runs against a warm `target/` are dominated by the
  per-mutation `cargo test --test properties` time (~0.6 s per
  mutation; build amortises across workers).
- Mutation count today (cargo-mutants 27.0.0): **181** under
  `patches-planner/src/`.

## Cleanup contract

The `tools/run-mutants.sh` wrapper installs `trap cleanup EXIT INT
TERM`. On every exit path (success, surviving mutations, test failure,
SIGINT, SIGTERM, per-mutation timeout) it removes:

- `mutants.out/` and `mutants.out.old/` at the repo root (cargo-mutants'
  primary log / diff / outcome tree);
- worker temp dirs `$TMPDIR/cargo-mutants-*.tmp` younger than 4 hours
  (best-effort sweep; cargo-mutants itself cleans these on normal exit).

`--keep-output` skips the trap so a triage iteration can inspect
`mutants.out/missed.txt`, the per-mutation logs under `mutants.out/log/`,
and the working diffs under `mutants.out/diff/` without re-running the
pass. Remove the directory manually when done.

**The cleanup contract is load-bearing.** A pass that leaves 30 GB of
worker copies in `target/` (or, on macOS, in `$TMPDIR`) silently steals
disk from local builds and from CI. Do not weaken `trap cleanup EXIT
INT TERM` without an explicit replacement.

## Worker mode: default (out-of-tree) chosen over `--in-place`

Resolves **E161 open question 1**.

| Mode               | 7-mutation wall-clock | Peak disk (mutants.out / TMPDIR)       |
| ------------------ | --------------------- | -------------------------------------- |
| `--in-place` jobs=1| 10 s                  | 216 KB (in mutants.out only)           |
| default workers j=2| 23 s                  | 360 KB mutants.out; transient TMPDIR copies cleaned per-mutation |

Both modes keep `target/` flat (baseline 10 GB, unchanged before /
after). `--in-place` is faster for tiny shards because it skips the
worker tree copy, but worker mode parallelises and amortises the copy
across all mutations on a long pass — at `jobs=4` a full pass is
estimated to complete in ~2-3 min vs ~5 min in-place.

Worker mode is also **safer on interrupt**: cargo-mutants modifies a
copy of the source under `$TMPDIR/cargo-mutants-*.tmp`, so Ctrl-C
cannot leave the working tree mid-mutation. `--in-place` modifies
`patches-planner/src/` directly; a hard interrupt during a mutation
would require `git checkout patches-planner/` to recover. The wrapper
adds that recovery for either mode would complicate the trap; sticking
to workers avoids the question.

Default jobs setting is `--jobs 4` (Justfile recipe → wrapper). Bump up
on a CI runner with more cores by overriding via:

```sh
just mutants --jobs 8
```

`--jobs` passed through `EXTRA` overrides the wrapper's `--jobs 4`
because the last `--jobs` on the command line wins.

## Triage workflow for a surviving mutation

`mutants.out/missed.txt` lists mutations that built and passed tests
without being killed. For each surviving mutation:

1. **Read the source line and replacement** from `missed.txt`.
2. **Decide**: is this mutation observable? A mutation that swaps an
   `==` for `!=` in a comparison whose outcome is later sorted away is
   genuinely equivalent.
3. **If observable:** add a covering test (preferentially a property in
   `patches-planner/tests/properties.rs`, falling back to a unit test
   under the module's `tests.rs`) and rerun `just mutants` on just that
   file via `--file 'path/to/file.rs'`.
4. **If equivalent:** annotate the function with `#[mutants::skip]`
   plus a one-line justification on the line above:

   ```rust
   // mutants::skip — order-independent: callers sort the result later.
   #[mutants::skip]
   fn unstable_iter_order(&self) -> Vec<NodeId> { ... }
   ```

5. Record the decision in the relevant pass-ticket close notes
   (0985 / 0986).

## CI: advisory nightly job

`.github/workflows/mutants.yml` runs `just mutants` on a nightly
schedule and uploads `mutants.out/` (via `--keep-output`) as a job
artefact. **Non-blocking**: a regression in mutation coverage is a
triage signal, not a merge gate. The job posts no comments — pull the
artefact to inspect.

Trigger manually with `gh workflow run mutants.yml` to re-run on demand
without waiting for the nightly schedule.
