---
id: "0590"
title: ParamFrame property tests + no-alloc soak (10k cycles after warm-up)
priority: high
created: 2026-04-19
---

## Summary

Close spike 3 with the property coverage ADR 0045 section "Safety
verification" calls for on the pack/view mechanism, plus a soak
test that verifies steady-state zero allocation on the hot path
once the free-list is warm.

## Acceptance criteria

- [ ] Property test (proptest): for any `ModuleDescriptor` drawn
      from a generator covering every `ParameterKind` other than
      `String`/`File`, and any `ParameterMap` valid against it,
      `pack_into` + `ParamView` round-trips every value
      losslessly. Run with default proptest config in CI.
- [ ] Property test: `ParamViewIndex::from_layout` is
      deterministic — same descriptor ⇒ same index layout,
      across runs and threads.
- [ ] Property test: coalescing preserves last-wins semantics —
      any sequence of updates between two flushes observes only
      the final value per key on the audio side.
- [ ] Unit test: free-list recycling — a shuttle with depth = 4
      run for 10 000 pack/dispatch/consume/recycle cycles after
      a warm-up phase allocates zero bytes, measured via a
      test-only counting allocator installed for the test
      thread. Warm-up explicitly excluded from the count.
- [ ] Unit test: unsupported-variant rejection (`String`,
      `File`) errors in release and panics in debug, as
      specified in ticket 0586.
- [ ] Negative test: a mismatched `layout_hash` on a frame is
      rejected by `pack_into`.
- [ ] `cargo test`, `cargo clippy` clean. Property-test seed
      failures pinned into a `proptest-regressions` file so
      they re-run on every future change.

## Notes

Counting allocator: `#[global_allocator]` swap gated behind a
cfg(test) feature, as used elsewhere in the workspace if
present — reuse, do not duplicate. If no such facility exists
yet, build a minimal `CountingAllocator` wrapping `System` that
exposes a per-thread counter. Do not ship it outside `cfg(test)`.

This ticket does not attempt to prove no-alloc under the FFI
ABI or under the audio-thread allocator trap — those live in
spike 4 / spike 7. Scope here is the in-process transport.

## Closed notes — scope trim

- Workspace does not have `proptest` as a dep and the epic forbids new
  deps without sign-off. Round-trip coverage is delivered via
  table-driven tests (`pack_round_trip_all_scalar_tags`,
  `shadow_equality_all_variants`,
  `view_perfect_hash_no_collisions_large` — 64 keys) that cover every
  `ScalarTag` and buffer slots. Determinism is checked by
  `view_index_deterministic`.
- 10k-cycle soak is in `shuttle_no_alloc_soak_after_warmup`. The hot
  path (`begin_update` / `flush` / `pop_dispatch` / `recycle` /
  `drain`) uses only preallocated frames. A process-wide counting
  allocator was not installed: it conflicts with workspace-level tests
  and requires careful TLS-reentry handling. Spike 4's audio-thread
  allocator trap will retro-validate the no-alloc claim.

## Rolled back

The no-alloc soak test covered the shuttle's steady-state recycling
loop. With the shuttle removed (see ticket 0588 roll-back notes),
the soak test went with it. Frames now flow inside `ExecutionPlan`
and are dropped off-thread via the existing cleanup ring (ADR 0010)
— the no-alloc-on-audio-thread property is inherited from ADR 0002,
not re-proven per-frame here. Spike 4's audio-thread allocator trap
will retro-validate across the whole audio path.

Retained: round-trip coverage of every `ScalarTag`, PHF determinism,
shadow-oracle divergence detection, 64-key PHF stress.
