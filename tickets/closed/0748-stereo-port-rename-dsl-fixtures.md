---
id: "0748"
title: Migrate patches-dsl test fixtures to ADR 0059 stereo ports
priority: medium
created: 2026-04-29
adrs: ["0059"]
epic: "E127"
depends_on: []
---

## Summary

`patches-dsl/tests/fixtures/**` contains ~40 `.patches` files that
still wire stereo modules through paired `in_left`/`in_right` /
`out_left`/`out_right` ports. ADR 0059 retired those names on symmetric
stereo modules; build now rejects them with
`"module 'X' has no input port 'in_left/0'; available inputs: [in/0]"`.

Mechanical rename: drop the paired cables and substitute a single
`mod.in` / `mod.out` cable. Mono→stereo broadcast (the default at the
cable layer) covers the case where a single mono source previously fed
both halves. A `StereoJoiner` is needed only where two genuinely
different mono sources fan into a stereo input (rare in fixtures).

## Acceptance criteria

- [ ] No fixture under `patches-dsl/tests/fixtures/**` references
      `in_left`/`in_right`/`out_left`/`out_right` on stereo modules.
      Allowed exceptions: `StereoSplitter`, `StereoJoiner`, and any
      explicit mid/side asymmetric processor.
- [ ] `cargo test -p patches-dsl` passes.
- [ ] Where a fixture was paired with a Rust-side cable-count
      assertion, the assertion is updated to reflect the collapsed
      cable count (one broadcast vs. two paired cables).

## Notes

`rg -- '\\.in_left|\\.in_right|\\.out_left|\\.out_right' patches-dsl/tests/fixtures`
gives the working set. `simple.patches` is the canonical smallest
case and is the example used in
`integration_tests::dsl_pipeline::flat_patch_round_trip` (which
ticket 0750 covers).
