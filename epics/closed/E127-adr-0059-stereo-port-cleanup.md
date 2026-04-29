---
id: "E127"
title: ADR 0059 stereo-port cleanup across fixtures, examples, and tests
created: 2026-04-29
tickets: ["0748", "0749", "0750", "0751"]
adrs: ["0059"]
---

## Goal

Finish the [ADR 0059](../../adr/0059-stereo-cables.md) migration on the
input side of the system: every `.patches` file and inline DSL string
under version control should use the single-`in`/`out` stereo
convention (with mono→stereo broadcast or explicit
`StereoSplitter`/`StereoJoiner`) instead of the retired
`in_left`/`in_right`/`out_left`/`out_right` paired ports.

ADR 0059 retired the paired ports on symmetric stereo modules and the
runtime code, registry, and module descriptors enforce the new shape.
Fixtures and example patches were not migrated in the same pass, so
many `cargo test -p patches-integration-tests` runs and most
`patches-vintage/examples` files panic at build time with
`"module 'X' has no input port 'in_left/0'; available inputs: [in/0]"`.
Surfaced while closing ticket 0747; the broadcast-flag fix unblocked
the FFI plumbing but left the fixture rot in place.

## Scope

1. **DSL fixtures** (`patches-dsl/tests/fixtures/**`). ~40 files
   reference `in_left`/`in_right`/`out_left`/`out_right` on `AudioOut`,
   `VChorus`, `FdnReverb`, `StereoMixer`, etc. Mechanical rename,
   except where a fan-in to two distinct stereo halves (e.g.
   `pad_l.out -> mix.in_left`, `pad_r.out -> mix.in_right`) needs an
   explicit `StereoJoiner`.

2. **Examples** (`examples/**`, including `examples/song1`,
   `examples/microtonal`). Same rename + occasional splitter/joiner
   inserts where mono effect chains live between two stereo blocks.

3. **Inline DSL strings in tests**. `patches-integration-tests/tests/`
   and `patches-dsl/tests/` contain multi-line raw-string fixtures
   embedded in Rust source. Update strings, plus the cable-count
   assertions in `dsl_pipeline::flat_patch_round_trip` etc. that
   change when paired cables collapse to one broadcast cable.

4. **Docs and CLAUDE.md content**. `examples/CLAUDE.md`, mdBook
   snippets, and module doc comments may still cite `_left`/`_right`
   port names; update for consistency.

## Acceptance

- `rg -- '\\.in_left|\\.in_right|\\.out_left|\\.out_right'`
  returns hits only inside `StereoSplitter`, `StereoJoiner`, and
  documented mid/side asymmetric processors (per ADR 0059).
- `cargo test --workspace` (or the inner-loop subset) passes without
  the `UnknownPort` build errors.
- No example loses audible behaviour; mono→stereo broadcast is
  semantically identical to a duplicated cable, so most edits are
  cosmetic. Where a splitter/joiner is inserted, audio output is
  identical to two-cable form.
- Any fixture-derived golden files (e.g. `vintage_baseline.f32`)
  regenerated via existing `--ignored regenerate_*` tests.

## Notes

The vintage examples (`patches-vintage/examples/*.patches`,
`patches-integration-tests/fixtures/vintage_baseline.patches`) were
already migrated in 0747; this epic covers the rest. `VStereoBbd`
landed in 0747 and is the canonical replacement for paired
`VBbd(channels: 1)` instances bracketing a stereo signal.

Pre-0747 failing tests captured for visibility:

- `alloc_trap::audio_tick_no_allocation_stereo_batch`
- `alloc_trap::audio_tick_performs_no_allocation`
- `dsl_pipeline::flat_patch_round_trip`
- `dsl_pipeline::template_expansion`
- `dsl_pipeline::nested_template_expansion`
- `dsl_pipeline::tap_target_expand_and_bind`
