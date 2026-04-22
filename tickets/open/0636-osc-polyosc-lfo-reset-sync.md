---
id: "0636"
title: Osc / PolyOsc / Lfo — reset_out and sync in
priority: medium
created: 2026-04-22
epic: "E103"
adr: "0047"
depends_on: ["0632", "0633", "0635"]
---

## Summary

Extend the standard oscillator sync pattern from `VDco` to the
remaining phase-accumulator oscillators: `Osc`, `PolyOsc`, and `Lfo`.
Same port shape (`reset_out` + `sync`), same sub-sample BLEP reset
logic adapted to each module's waveforms.

## Acceptance criteria

- [ ] `Osc` (`patches-modules/src/oscillator.rs`) gains `reset_out`
      and `sync`. Sub-sample BLEP applied to saw and square outputs;
      sine and triangle simply snap (band-limited already, continuous
      at reset if read at correct phase).
- [ ] `PolyOsc` same pattern, per-voice.
- [ ] `Lfo` (`patches-modules/src/lfo.rs`) gains `reset_out` and
      `sync`. BLEP optional (LFO rates are well below audible
      aliasing); at minimum, do a clean phase reset. Document the
      choice in the module doc comment.
- [ ] Existing `Lfo` `sync` input (if any under ADR 0030 0.5-threshold
      convention) is either removed in favour of the typed `sync`, or
      renamed for clarity. Confirm in PR description which path was
      chosen and why.
- [ ] Unit tests per module: reset_out frac, sync reset correctness,
      BLEP where applicable.
- [ ] Module doc comments updated to list the new ports (follow
      CLAUDE.md "Module documentation standard").

## Notes

Consider extracting the shared sync-BLEP reset helper from 0635's
`VDco` work into `patches-dsp` (or a common file in
`patches-modules/src/common/`) before duplicating it here.
