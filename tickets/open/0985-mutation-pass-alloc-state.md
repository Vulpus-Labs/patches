---
id: "0985"
title: Mutation testing pass — state/alloc + state/mod
priority: medium
created: 2026-05-29
---

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
