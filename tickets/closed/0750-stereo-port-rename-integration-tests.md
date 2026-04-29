---
id: "0750"
title: Migrate inline DSL strings in integration tests to ADR 0059 stereo ports
priority: medium
created: 2026-04-29
adrs: ["0059"]
epic: "E127"
depends_on: ["0748"]
---

## Summary

`patches-integration-tests/tests/dsl_pipeline.rs` and
`patches-integration-tests/tests/alloc_trap.rs` carry inline raw-string
fixtures that still wire `out.in_left` / `out.in_right` on `AudioOut`
and other stereo modules. Tests that fail today on this pre-existing
rot:

- `alloc_trap::audio_tick_no_allocation_stereo_batch`
- `alloc_trap::audio_tick_performs_no_allocation`
- `dsl_pipeline::flat_patch_round_trip` (uses `simple.patches`)
- `dsl_pipeline::template_expansion`
- `dsl_pipeline::nested_template_expansion`
- `dsl_pipeline::tap_target_expand_and_bind`

Update the inline DSL to use single-`in`/`out` ports, and adjust the
`flat_patch_round_trip` cable-count expectation (was 2 paired cables;
becomes 1 broadcast cable).

## Acceptance criteria

- [ ] Every failing test in the list above passes.
- [ ] No `in_left`/`in_right`/`out_left`/`out_right` strings remain in
      `patches-integration-tests/tests/**` outside legitimate uses
      (`StereoSplitter`/`StereoJoiner` and asymmetric processors).
- [ ] `cargo test -p patches-integration-tests` passes (modulo any
      pre-existing failures unrelated to ADR 0059, captured separately).

## Notes

Depends on 0748 because `simple.patches` rename lands there;
`flat_patch_round_trip` reads it via `load_fixture` and asserts the
edge count.
