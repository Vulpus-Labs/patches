---
id: "0947"
title: LSP hover + manual + goldens for the unified step grammar
priority: low
created: 2026-05-20
epic: E153
depends_on: ["0946"]
---

## Summary

Polish layer for E153: update LSP hover to recognise the new cell
forms and surface the resolved `StepEffect` per cell; rewrite the
manual's tracker section against the unified model; add a battery
of golden / integration tests covering the new combinations.

After this ticket the new authoring surface is documented, hoverable,
and regression-guarded.

## Acceptance criteria

### LSP

- [ ] `tree_nav::classify_step_node` extended to recognise the new
      cells:
  - `step_step_to` (`/value`) → new `CursorContext::StepCv`.
  - `step_slide_open` (`value>`) → new `CursorContext::StepSlideOpen`.
  - `step_tie_flow` (`>_`) → new `CursorContext::StepTieFlow`.
  - `step_slide_close` (`>value`) → new
    `CursorContext::StepSlideClose`.
  - Existing `step_tie` (now `_`) keeps `CursorContext::StepTie`.
  - Existing `step_repeat` (`*N`) keeps
    `CursorContext::StepRepeat`.
- [ ] `hover/step.rs` extended with handlers for each new context.
      Hover text states:
  - what this cell emits (trigger / slide / step / hold),
  - the channel's slide-open status going in (computed by
    re-running `resolve_step_effects` over the row),
  - the resolved `StepEffect` variant.
- [ ] LSP completions pass-through arm gains the new variants
      (mirror the 0941 change for `StepTie`/`StepRepeat`).
- [ ] LSP test in `patches-lsp::hover::tests`:
  - `hover_on_step_to` — cursor on `/G4` reports "step cv to G4,
    no retrigger."
  - `hover_on_slide_open` — cursor on `C4>` reports "trigger +
    open slide; closes at next non-`_` cell."
  - `hover_on_slide_flow` — cursor on `>_` reports "slide flow."
  - `hover_on_slide_close_in_tick` — cursor on `>G4` reports
    "ramp to G4 within this tick, no retrigger."
  - `hover_on_bare_value_after_open_slide` — cursor on `G4` in
    `C4> _ G4` reports "fresh trigger; preceding slide closes at
    boundary at G4."

### Corpus

- [ ] `patches-lsp/tests/syntax_corpus/tracker_step_event_grammar.corpus`
      (introduced in 0946) covers every cell form. Already
      acceptance-tested for parse shape; this ticket adds entries
      for cross-cell composition (slide + roll, slide + cv2,
      `>>` cv2 slide target, etc.) to lock in the surface area.

### Manual

- [ ] `docs/src/dsl-reference.md`: replace the "Step notation" +
      "Slides" + "Repeats" + "Ties — sustain vs roll continuation"
      sections with a unified "Step events" section organised by
      the cell taxonomy from ADR 0077. Include a worked-example
      table showing each five-tick figure rendered against the
      sample timeline (lead-in shape, trigger placement, gate
      articulation).
- [ ] `docs/src/modules/tracker.md`: PatternPlayer "Slides,
      repeats, and tie-spread rolls" section rewrites to reference
      the unified surface; the existing tie-spread sub-heading
      from ticket 0942 is folded into the unified description.
- [ ] Cross-link the new section from
      `docs/src/SUMMARY.md` if anchor names changed.

### Audio goldens

- [ ] `patches-integration-tests::tracker` gains coverage for
      every example in ADR 0077 § "Continuation absorption", with
      explicit sample-offset assertions:
  - `E4 /G4` — two-tick figure; trigger at sample 0 only; cv
    steps to G4 at tick-1→tick-2 boundary; gate held both ticks.
  - `E4 _ /G4` — three ticks; trigger at sample 0; cv steps to G4
    at tick-2→tick-3 boundary.
  - `E4> _ G4` — three ticks; trigger at sample 0; ramp through
    ticks 1+2 ending exactly at G4 at tick-2 boundary; fresh
    trigger at start of tick 3.
  - `E4 _ >_ /G4` — four ticks; trigger at sample 0; flat ticks
    1+2; ramp tick 3; hold G4 tick 4.
- [ ] At least one swung-pattern golden exercising the
      sub-event-schedule fix from 0945, with sample offsets
      hand-computed against per-tick swung durations.
- [ ] `just push` green.

### Memory

- [ ] Save the three core design rules from the epic notes as
      separate memory entries:
  - `feedback_tracker_close_rule.md` — "bare `value` is always
    fresh trigger; lead-in shape is prior-cell business; close
    cell carries the slide's endpoint."
  - `feedback_tracker_continuation_absorption.md` — "`*N` and
    open slides both absorb bare `_`; modifier on the anchor
    decides; `>_` is the explicit-flow form for slides starting
    on non-value cells."
  - `feedback_tracker_value_gt_value_sugar.md` — "`C4>E4 _` ≡
    `C4> /E4`; the single-tick slide-then-hold sugar is the
    common shorthand; both lower to the same `StepEffect`s."

## Notes

- The LSP hover should call the same `resolve_step_effects` helper
  used at row-build time, not re-implement classification. Reuse
  is the whole point of the StepEffect indirection.
- Manual tables get the `tools/align-tables.py` pass before commit.
- The audio goldens in this ticket are *integration* tests with
  RMS / trigger-offset assertions in the style of the existing
  `patches-integration-tests::tracker` suite — they are not raw
  WAV bit-equality tests (none of the existing suite is).
