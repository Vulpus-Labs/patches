---
id: "0944"
title: PatternPlayer dispatches on StepEffect
priority: medium
created: 2026-05-20
epic: E153
depends_on: ["0943"]
---

## Summary

Rewrite `PatternPlayerCore::apply_step` to dispatch on
`step.effect: StepEffect` (introduced in 0943) instead of inspecting
`step.trigger`, `step.gate`, `step.cv1_end`, `step.repeat`,
`step.repeat_span`, and `step.absorbed_by_roll`. Drop the per-flag
if-else cascade in favour of one `match step.effect` per channel.

This is a refactor: audio output is **bit-identical** to before.
Existing tests (including the E152 spread suite from 0939/0940) must
pass unchanged, including the
`spread_span_1_is_bit_identical_to_pre_e152` regression guard.

The legacy fields stay on `TrackerStep` for now (they're set by the
row-build pass alongside `effect`). They're not read by anyone after
this ticket; ticket 0946 removes them.

## Acceptance criteria

- [ ] `PatternPlayerCore::apply_step` body is a `match
      step.effect` covering every `StepEffect` variant:
  - `Silent` → drop gate + trigger, clear slide/repeat.
  - `StartNote { cv1, cv2, slide, roll }` → trigger + write cv +
    open slide/roll arms as the existing code does today.
  - `StepCv { cv1, cv2 }` → write cv, gate stays, no trigger, close
    any open slide at boundary.
  - `Hold` → gate stays, cv unchanged, no trigger.
  - `SlideFlow` → keep gate; if a slide is open, advance one
    sample (E152's existing "rising-edge sample counts" logic).
  - `SlideCloseInTick { cv1 }` → start a fresh slide whose target
    is `cv1`, completing within this tick.
  - `AbsorbedRoll` → preserve in-flight roll state; advance one
    sample (existing E152 path).
- [ ] State-reset boilerplate (`slide_active = false`,
      `repeat_active = false`, `repeat_gate_off_at = f32::MAX`,
      etc.) is centralised — each effect branch sets only the
      flags it owns, no duplication across arms.
- [ ] All pre-existing pattern-player tests pass unchanged:
  - `apply_step_note_event_sets_cv_gate_trigger`
  - `tick_without_edge_holds_previous_values`
  - `trigger_edge_detect_fires_once_per_rising_edge`
  - `tie_holds_gate_and_carries_cv`
  - `slide_interpolation_sets_ramp_state`
  - `repeat_subdivision_schedules_sub_triggers`
  - all `spread_*` tests from 0940
  - `channel_count_mismatch_*`
  - `stop_sentinel_clears_all`
- [ ] All integration tests in `patches-integration-tests::tracker`
      pass unchanged (slides, repeats, tie-spread, sustain ties,
      pattern switching, loop, swing).
- [ ] `just commit -p patches-modules -p patches-interpreter` green.

## Notes

- This is the largest single internal refactor of the epic. The
  acceptance bar is *bit-identical output* — any drift indicates
  the effect resolution in 0943 doesn't quite match the existing
  apply_step branches. Fix in 0943 and rebase; don't paper over
  with branch-specific tweaks here.
- Drop the inter-tick `advance_roll_one_sample` helper's coupling
  to `step.absorbed_by_roll`; the effect kind tells you whether to
  advance.
- After this ticket the *only* remaining use of `trigger`, `gate`,
  `cv1_end`, `cv2_end`, `repeat`, `repeat_span`, and
  `absorbed_by_roll` on `TrackerStep` is the row-build pass
  populating them and reading them to produce `effect`. Ticket
  0946 removes them entirely once the new grammar lands.
