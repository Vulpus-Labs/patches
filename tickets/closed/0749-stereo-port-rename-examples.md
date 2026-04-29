---
id: "0749"
title: Migrate examples/*.patches to ADR 0059 stereo ports
priority: medium
created: 2026-04-29
adrs: ["0059"]
epic: "E127"
depends_on: []
---

## Summary

Several example patches under `examples/` (notably `pad.patches`,
`radigue_drone.patches`, `microtonal/microtonal.patches`,
`song1/song.patches`, `song1/drum_machine.patches`) still reference
the retired `_left`/`_right` port names on symmetric stereo modules.
These compile-fail under the current registry. Update to the
single-`in`/`out` convention with mono→stereo broadcast or explicit
`StereoSplitter`/`StereoJoiner` where the signal flow legitimately
splits and rejoins around mono effects.

## Acceptance criteria

- [ ] No file under `examples/` (excluding `patches-vintage/examples`,
      already migrated in 0747) references `in_left`/`in_right`/
      `out_left`/`out_right` on stereo modules.
- [ ] Each updated example loads via `patches-player` without build
      errors. Smoke-load via the existing example-runner test or a
      new one as needed.
- [ ] Audio output is unchanged where the rename is purely the
      collapse of duplicate-cable broadcast. Where a splitter/joiner
      is introduced to keep mono effects in the chain, listening test
      against the previous behaviour is enough — no golden capture
      required for examples.

## Notes

`patches-vintage/examples/*.patches` and the
`vintage_baseline.patches` integration fixture are already migrated
(ticket 0747). Use those as templates — including the `VStereoBbd`
substitution where a paired-mono `VBbd` ran on each channel.
