---
id: "0951"
title: "EnvCore: multi-stage envelope core (patches-dsp)"
priority: medium
created: 2026-05-22
epic: E155
---

## Summary

Add a pure, alloc-free multi-breakpoint envelope state machine to `patches-dsp`,
sibling to the existing `AdsrCore` (`patches-dsp/src/adsr.rs`). Unlike ADSR's
fixed 4-stage shape, this runs an arbitrary number of `(target_level, time,
curve)` stages with a designated sustain stage, plus a release tail — enough to
express the D50-style contours ADSR cannot (attack spike → dip → secondary swell
→ sustain).

This ticket is the kernel only — no module, no ports. Time-scaling (key-follow)
and velocity scaling are applied by the *module* (0952) ahead of the core; the
core consumes already-scaled inputs so it stays generic and testable.

## Acceptance criteria

- [x] `EnvCore` in a new `patches-dsp/src/multistage_env.rs`, re-exported from
      the crate root.
- [x] Fixed-capacity stage storage (`[Stage; MAX_STAGES]` + `len`, `MAX_STAGES =
      8`), **no heap allocation**, so it is audio-thread safe.
- [x] `Stage { target_level, time_secs, curve }`; `curve` reuses `AdsrShape`
      (`Linear`/`Exponential`) directly — the enum was clean to share.
- [x] A designated **sustain stage index**: holds at that stage's level while
      gate is high, then runs stages `sustain_stage+1..len` as the release tail
      on gate-off. (No stage past the sustain stage → idles at current level;
      designate a final `target = 0.0` stage for a release to silence.)
- [x] `tick(&mut self, triggered: bool, gate_high: bool, time_scale: f32) -> f32`
      returning clamped `[0, 1]`. `time_scale` multiplies all stage times per
      tick via a phase accumulator, so it may vary per tick (bends).
- [x] Re-trigger during any stage restarts from the **current level** (`seg_start
      = level`), matching `AdsrCore` re-trigger semantics.
- [x] Velocity acts on stage *levels*: `set_level_scale` is latched at trigger
      into `latched_level_scale` and multiplies every stage target. **Split
      decision (with 0952):** the module computes a velocity→scale curve and
      pushes it via `set_level_scale`; the core latches at trigger so an
      in-flight envelope keeps a stable scaling. Key-follow stays separate as the
      per-tick `time_scale` arg.
- [x] Unit tests: single-stage linear ramp; sustain hold under gate; release
      tail on gate-off; `time_scale` halving/doubling; re-trigger from mid-level;
      gate-off mid-attack → release; exponential curve; degenerate cases
      (zero-time stage = instant jump, `len == 0`); reset determinism.

## Notes

- Model on `patches-dsp/src/adsr.rs` `AdsrCore` for stage-machine style,
  clamping, and `Linear`/`Exponential` segment shapes — mirror its conventions.
- Keep `time_scale` as a plain multiplier so the module can fold key-follow *and*
  any global rate scaling into one number; the core needn't know about pitch.
- `MAX_STAGES`: pick a small fixed cap (8 likely enough for D50-style contours).
  It becomes the descriptor count-axis bound in 0952.
- No RT allocation, no `unwrap`/`expect` (library code).
- Validation: `just inner -p patches-dsp`.
