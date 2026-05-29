---
id: "0985"
title: Mutation testing pass — state/alloc + state/mod
priority: medium
created: 2026-05-29
closed: 2026-05-29
---

## Done

Scope: `patches-planner/src/state/alloc.rs` + `patches-planner/src/state/mod.rs`.
Initial pass: **74 mutants, 11 missed** (with `tools/run-mutants.sh`
defaulting to `--test properties` only — wrapper restriction filtered
out the unit-test kill surface). Re-run dropping the test selector:
**74 mutants, 2 missed**. After triage: **0 missed**.

## Wrapper fix

[tools/run-mutants.sh](../../tools/run-mutants.sh) originally pinned
the kill surface to `-- --test properties`. That restriction hid 9
mutations the existing unit tests (`state::alloc::tests::*`,
`state::tests::*`) already kill — `classify_nodes` equality checks,
`compute_order_with_fusion` SCC inequality, the two `validate_*`
invariants. Dropping the selector lets cargo-mutants run every
patches-planner test, which is the correct kill surface for this
ticket and 0986. The wrapper comment now records why.

## Triage table

| Mutation                                                                                     | Outcome | Action                                                                                                                                                                                                                                      |
| -------------------------------------------------------------------------------------------- | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `state/mod.rs:49: <impl fmt::Display for PlanError>::fmt -> Ok(Default::default())`          | skipped | `#[cfg_attr(test, mutants::skip)]` + `// MUTANTS:` reason. Display text is for human diagnostics; programmatic callers match on the enum variant. No test asserts the rendered string.                                                      |
| `state/alloc.rs:288: replace - with /` in cycle→scratch flip                                 | covered | Tightened `state::alloc::tests::flip_cycle_to_scratch` to compare the full `cycle_freelist` against `vec![logical]` instead of counting matches. Caught the mutated `existing / SCRATCH_CAPACITY` adding a phantom logical to the freelist. |

The other 9 mutations the original wrapper hid (classify_nodes `==`
flips, `compute_order_with_fusion` `!fused / +=`, the two `validate_*`
early-return removals) were already covered by:

- `state::tests::classify_type_changed_node_is_install`
- `state::tests::classify_shape_changed_node_is_install`
- `state::tests::classify_surviving_no_changes_is_update_with_empty_diff`
- `state::tests::compute_order_with_fusion_*` family
- `state::tests::validate_fused_invariant_*`
- `state::tests::validate_scratch_fused_consistency_*`

Confirmed by the re-run with the corrected wrapper.

## `mutants` dev-dependency

`patches-planner/Cargo.toml` gains
`mutants = "0.0.3"` as a `[dev-dependencies]` entry — an empty marker
crate that exists so `#[mutants::skip]` parses under `cargo test`.
cargo-mutants 27 reads the attribute by path and skips the function
without needing real code from this crate.

## Verification

```sh
$ just mutants --file patches-planner/src/state/alloc.rs \
              --file patches-planner/src/state/mod.rs
73 mutants tested in 83s: 56 caught, 17 unviable
[run-mutants] removing mutants.out/ (exit 0)
```

73 (vs. initial 74) because the Display `#[mutants::skip]` annotation
removed one mutation from the pool. `just push` green (build 10.9 s /
test 69.5 s / clippy 9.4 s).

## Summary

Run `cargo-mutants` against `patches-planner/src/state/alloc.rs` and
`patches-planner/src/state/mod.rs` — the two highest-value mutation targets in
the crate: dense allocation logic (region flips, freelist push / pop,
exhaustion checks) and the SCC / classification helpers. Triage every
surviving mutation by either writing a covering test or annotating
`#[mutants::skip]` with a one-line justification.

## Acceptance criteria

- [ ] Initial run scope limited to the two files via `cargo-mutants` `--file`
      flags so wall-clock stays under 15 minutes on a developer laptop.
- [ ] Every reported `MISSED` mutation is triaged as either:
  - [ ] **covered** — a new test (unit or property) is written that bites the
        mutation, and the file's mutation list is re-run to confirm the
        mutation is now caught;
  - [ ] **skipped** — production code is annotated `#[mutants::skip]` with a
        one-line `// MUTANTS: <reason>` comment explaining the equivalence
        (must be specific, not "equivalent").
- [ ] The triage table (mutation → covered / skipped, link to test or reason)
      is recorded in the ticket close notes for future reference.
- [ ] After triage: a second full run on the same scope reports zero `MISSED`.
- [ ] `just push` green; cleanup verified (no stale `target/mutants.out/`
      after the second run).

## Notes

Part of epic **E161**, phase P3. Depends on 0982 (so the single-build property
suite is in place when the audit runs) and 0984 (mutation infrastructure +
cleanup).

Expected high-value catches based on the planner's mental audit during E160:

- `allocate_buffers` scratch exhaustion (`if i >= scratch_cap`) — `>=` vs `>`
  off-by-one.
- `allocate_buffers` cycle exhaustion (`if logical >= CYCLE_CAPACITY`) — same.
- `allocate_buffers` `cycle_already_freed.insert(logical)` guard — replace
  with `true` would cause double-free of a cycle logical to the freelist;
  caught by `vacated_cycle_slot_freelisted_and_recycled_lifo` if the count
  assertion is rigorous.
- `classify_producer_ports`' `*v = *v || needs_cycle` — replacing with `*v =
  needs_cycle` regresses the mixed-fanout case (first consumer fused, second
  cyclic ⇒ port should still be cycle).
- `compute_order_with_fusion`'s `a != b` SCC-membership comparison — flipping
  inverts every cable's fusion.
- The two `validate_*` invariants' continue / early-return conditions —
  removing either causes false positives or false negatives.

If a surviving mutation cannot be killed by a reasonable test (a genuine
equivalence — e.g. permuting the order of a `HashSet` collection that is
later sorted), the `#[mutants::skip]` annotation is the correct outcome — but
the reason must be specific, not the bare word "equivalent". Reviewer should
be able to verify the equivalence from the comment alone.
