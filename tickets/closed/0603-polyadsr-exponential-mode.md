---
id: "0603"
title: ADSR / PolyADSR — exponential envelope shape
priority: medium
created: 2026-04-20
epic: E102
---

## Summary

Add an exponential shape mode to both `ADSR` (mono) and `PolyADSR`.
Current linear-ramp segments sound synthetic on percussive material;
analog RC envelopes give the characteristic pluck shape and natural
release tail. Needed by the Juno voice demo (E102) and generally
useful outside it. Both mono and poly variants get the parameter for
consistency — they share the `patches-dsp::adsr` core, so the work
is in the core plus two wrapper param rows.

## Design

### Parameter

- New `shape: enum { Linear, Exponential }` parameter on both `ADSR`
  and `PolyADSR`.
- Default `Linear` — preserves existing behaviour for all current
  patches.
- Use `params_enum!` so module code reads `Shape::Exponential`.

### Segment update rule

Linear (unchanged): `y += rate` per sample.

Exponential: `y += k * (target - y)` per sample, where `k` is derived
from the slider-time such that the segment reaches a given
fraction of target over the specified duration (standard RC
convention: ~63% per time constant, ~99% over ~5τ — pick a convention
matching slider feel; document in code).

Per segment:

| Segment   | Target                             | Notes                                          |
| --------- | ---------------------------------- | ---------------------------------------------- |
| Attack    | `1.2 × peak`, clamp output at peak | Analog overshoot trick; constant, not a param. |
| Decay     | `sustain`                          | Natural RC approach.                           |
| Sustain   | —                                  | Held.                                          |
| Release   | `0.0`                              | From current level, not peak.                  |

State machine unchanged — only the per-sample update differs.

### Constants

- Overshoot factor `1.2` — hard-coded, documented as analog-emulation
  convention.
- Time-constant convention chosen to make slider feel match the
  linear mode (so a user switching shapes on a patch doesn't get
  wildly different overall timing). Document once picked.

## Acceptance criteria

- [ ] `shape` parameter exposed through DSL on `ADSR` and `PolyADSR`,
      defaults `Linear`.
- [ ] Existing patches with no `shape` set produce bit-identical
      output vs pre-change (regression guard).
- [ ] Exponential decay/release produce audible analog-style pluck
      and natural tail.
- [ ] Attack with `shape = Exponential` reaches peak and clamps; no
      overshoot visible at output.
- [ ] Unit tests for both modes covering A / D / S / R transitions.
- [ ] `cargo clippy` and `cargo test` clean.
- [ ] Doc comments on `ADSR` and `PolyADSR` updated with the new
      parameter row.

## Notes

- Core lives in `patches-dsp::adsr`; this adds a per-segment branch
  there and plumbs the param through both the `ADSR` and `PolyADSR`
  wrappers in `patches-modules`.
- Possible follow-up (out of scope here): a `curve` float in
  `[0, 1]` blending linear → exponential. Not needed for the Juno
  work; don't build speculative surface.
