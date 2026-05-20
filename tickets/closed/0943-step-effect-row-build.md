---
id: "0943"
title: StepEffect enum + channel-stateful row-build pass
priority: medium
created: 2026-05-20
epic: E153
---

## Summary

Introduce the `StepEffect` enum and a channel-stateful row-build
pass that resolves each AST step into one `StepEffect`. Land it
alongside the existing E152 annotation (`repeat_span`,
`absorbed_by_roll`) without removing the existing fields yet — this
ticket is the new pipeline; ticket 0944 wires the pattern player
to consume it.

Today the row-build pass (`patches-interpreter::tracker::build_tracker_data`)
calls `convert_step` per cell + `annotate_repeat_spans` per channel.
The new pass runs *also* per channel, walking left-to-right, and
emits one `StepEffect` per cell. The grammar surface is unchanged
in this ticket; the pass infers effects from the *current* AST
shape (trigger/gate/cv1_end/repeat/absorbed_by_roll).

The output is attached to the runtime `TrackerStep` as a new
`effect: StepEffect` field. The pattern player keeps reading the
old fields until ticket 0944 flips it over.

## Acceptance criteria

- [ ] `patches_core::tracker` gains the `StepEffect`, `SlideOpen`,
      and `RollSpec` types defined in ADR 0077 § "Row-build pass:
      StepEffect" with `Debug`, `Clone`, `PartialEq`.
- [ ] `TrackerStep` gains `effect: StepEffect` (default
      `StepEffect::Silent` for the default rest step).
- [ ] New helper `patches_core::tracker::resolve_step_effects(&mut
      [Step])` walks one channel's step run in source order and
      sets each `step.effect` to the resolved value. Channel-stateful
      (tracks slide-open / roll-active during the walk). Idempotent
      (clearing + re-resolving yields the same output).
- [ ] `build_tracker_data` calls `resolve_step_effects` per channel
      after `annotate_repeat_spans`. Both annotations coexist until
      0944.
- [ ] Unit tests in `patches-core::tracker::tests`:
  - `value` cell → `StartNote { cv1, slide: None, roll: None }`.
  - `value*N` cell with `repeat_span = 1` → `StartNote { roll: Some
    (RollSpec { count: N, span: 1 }), .. }`.
  - `value*N` with `repeat_span = S > 1` → same with span = S; the
    `S − 1` absorbed-roll cells get `AbsorbedRoll` effects.
  - `~` after a plain note → `Hold` effect.
  - `~` after a sliding head step (E152 slide tail) →
    `SlideFlow` effect.
  - `.` rest → `Silent`.
  - Slide head (`trigger=true, gate=true, cv1_end=Some`) →
    `StartNote { slide: Some(SlideOpen { close_cv1: end,
    closes_at_boundary: true }), .. }` for a one-tick slide.
- [ ] Integration tests in `patches-interpreter::tests::song_sequencer`:
  - `slide(2, A4, C5)` macro lowers to `StartNote` head + one
    `SlideFlow` tail with the same close target.
  - `x*3 ~ ~` channel produces `StartNote { roll }` + 2 `AbsorbedRoll`
    cells, matching the existing `repeat_span` annotation.
- [ ] `just inner -p patches-interpreter` green.

## Notes

- This ticket is **purely additive**. No existing behaviour
  changes. The runtime still reads the old fields. The new
  `effect` field is set but unused.
- The pass cannot resolve the new `_` / `/value` / `>value` / `>_`
  / `value>` cells yet because the grammar doesn't accept them.
  Resolution rules for those are added in ticket 0946 in the same
  helper.
- Memory note worth saving on close: `resolve_step_effects` is the
  single authoritative resolution of surface cell shape → runtime
  semantics. Future grammar extensions should add cases to this
  helper, not parallel resolution paths in the pattern player.
