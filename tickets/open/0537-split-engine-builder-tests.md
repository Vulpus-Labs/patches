---
id: "0537"
title: Split patches-engine builder/tests.rs by category
priority: medium
created: 2026-04-17
epic: E090
---

## Summary

[patches-engine/src/builder/tests.rs](../../patches-engine/src/builder/tests.rs)
is 712 lines covering: buffer pool indexing / exhaustion / freelist,
graph-build entry points (single module, fanout), and planner-related
behaviour (stable buffer index across replans, plan tick correctness).

## Acceptance criteria

- [ ] Convert to stub `src/builder/tests.rs` declaring a submodule
      tree under `src/builder/tests/`.
- [ ] Category split (final naming the ticket's call):
      - `pool.rs` — `pool_index_for`, `make_buffer_pool`,
        `freelist_recycles_indices_*`, `pool_exhausted_error_*`
      - `graph_build.rs` — `sine_to_audio_out_graph`,
        `fanout_buffer_shared_*`, `tick_runs_without_panic`,
        `input_scale_is_applied_*`
      - `planner.rs` — `stable_buffer_index_for_unchanged_module_across_replan`
        and other replan-stability tests
- [ ] Shared fixture builders (`default_registry`, `default_env`,
      `default_builder`, `default_build`, `p`, `hz_to_voct`) in
      `tests/mod.rs` or `tests/support.rs`.
- [ ] `cargo test -p patches-engine` passes with the same test count.
- [ ] `cargo build -p patches-engine`, `cargo clippy` clean.

## Notes

E090. Coordinate with E089 ticket 0513: the planner is moving into
`patches-planner`. If 0513 lands first, some of `planner.rs` category
tests may follow the planner into the new crate's test tree — do this
ticket's split either in whichever crate the tests end up in, or
sequence after 0513.
