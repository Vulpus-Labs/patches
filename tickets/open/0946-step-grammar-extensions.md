---
id: "0946"
title: Grammar + parser — `_`, `/value`, `value>`, `>_`, `>value`
priority: medium
created: 2026-05-20
epic: E153
depends_on: ["0945"]
---

## Summary

Land the surface-grammar changes from ADR 0077:

- Replace `~` with `_` for the tie token in pattern rows.
- Add `/value` (instant cv change, no trigger).
- Add `value>` (trigger + open slide).
- Add `>_` (slide flow / open from current cv).
- Add `>value` (slide close within tick).

Migrate all in-tree `.patches` fixtures and inline test patterns
from `~` to `_`. After this ticket the `~` token is rejected by
the step grammar.

Extend `resolve_step_effects` (from 0943) to map the new cell
shapes to the right `StepEffect` variants, applying the **unified
close rule**: a `StartNote`, `StepCv`, or `SlideCloseInTick` cell
closes any open slide; absorbed `_` / `>_` cells flow.

The runtime is ready (0944 + 0945); this ticket exposes the
authoring surface.

## Acceptance criteria

### Grammar

- [ ] `patches-dsl/src/grammar.pest`:
  - `step_tie` token renamed to `_` (was `~`).
  - New tokens: `step_step_to = ${ "/" ~ step_primary }`,
    `step_slide_open = ${ step_primary ~ ">" ~ !step_primary }`,
    `step_tie_flow = @{ ">_" }`, `step_slide_close = ${ ">" ~
    step_primary }`.
  - `step` rule extended to accept all the above.
  - Existing `value>value` form (`step_valued` with
    `step_slide_target`) keeps working as sugar.
- [ ] `patches-lsp/tree-sitter-patches/grammar.js` mirrored.
  Tree-sitter generated parser regenerated and committed.

### AST + parser

- [ ] `patches_dsl::ast::Step` either grows a `kind: StepKind` tag
      (encoding the cell shape) or — preferred — keeps the same
      fields with semantics resolved entirely by
      `resolve_step_effects` in row-build. Whichever route, the
      parser fills enough of `Step` for the row-build pass to
      classify correctly.
- [ ] Parser updates in `patches-dsl/src/parser/steps_songs.rs` to
      emit the new cell shapes.
- [ ] `resolve_step_effects` handles the new shapes per ADR 0077
      § "Continuation absorption" and § "Unified close rule":
  - `_` after a `*N` anchor → `AbsorbedRoll` (E152 path).
  - `_` after a `value>` or `>_` → `SlideFlow`.
  - `_` after a plain note → `Hold`.
  - `value` after an open slide → `StartNote { cv1, .. }` *and*
    the open slide closes at boundary at this value (the row-
    build pass also flags the slide head's `SlideOpen.close_cv1`
    to this value).
  - `/value` after an open slide → `StepCv { cv1, .. }`; slide
    closes at boundary at this value.
  - `>value` after an open slide → `SlideCloseInTick { cv1 }`;
    slide head's close target = `cv1`, `closes_at_boundary = false`.
  - `>_` opens a slide if none is open; otherwise flows.

### Cleanup

- [ ] `TrackerStep` drops `cv1_end`, `cv2_end`, `repeat`,
      `repeat_span`, `absorbed_by_roll`. The runtime reads
      `effect` only.
- [ ] `annotate_repeat_spans` (from ticket 0939) is removed; its
      logic is subsumed by `resolve_step_effects`. Its tests are
      ported or replaced with tests on the new pass.
- [ ] `slide(n, start, end)` macro continues to expand correctly,
      now lowering through `resolve_step_effects` rather than
      writing `cv1_end` directly. The expansion can be simplified
      to a sequence of `value>` + `>_` + `>value` shapes in the
      flat AST (or kept as a direct effect-emission shortcut).

### Migration

- [ ] All in-tree `.patches` files migrated: `~` in step rows →
      `_`. Sweep with `git grep -l '~' -- '*.patches'` and a
      careful per-file edit (cable-endpoint `~tap` / `~name`
      uses must NOT be touched). One-shot migration script under
      `tools/` is fine if it's run + the result committed.
- [ ] Inline test patterns in `patches-modules`,
      `patches-interpreter`, and `patches-integration-tests`
      migrated.
- [ ] Syntax corpus (`patches-lsp/tests/syntax_corpus/`):
      `tracker_tie_spread.corpus` (from 0941) updated to use `_`.
      New corpus file `tracker_step_event_grammar.corpus` covers
      the new shapes (one entry per cell form + each close-rule
      combination).

### Tests

- [ ] Parser tests in `patches-dsl` for each new cell shape.
- [ ] Row-build tests in `patches-core::tracker` for each
      `StepEffect` arising from the new shapes, including the
      examples from ADR 0077 § "Continuation absorption":
  - `E4 _ >_ /G4`
  - `E4> _ /G4`
  - `E4> _ >G4`
  - `E4> _ G4`
  - `E4 /G4`
  - `C4>E4 _` ≡ `C4> /E4` (assert effect equivalence)
- [ ] Integration tests in `patches-integration-tests::tracker`:
  - `slide_two_ticks_no_retrigger` — `E4> _ /G4` through the
    audio engine: trigger at sample 0 only, ramp completes by
    tick-2 boundary, gate held through tick 3.
  - `slide_three_ticks_full_ramp` — `E4> _ >G4`: ramp continues
    through all 3 ticks, one trigger at sample 0.
  - `slide_two_ticks_then_retrigger` — `E4> _ G4`: slide ends at
    tick boundary, fresh trigger on tick 3 at G4.
  - `step_cv_no_trigger` — `E4 /G4 _`: trigger at sample 0
    (E4), no second trigger, cv jumps to G4 at tick 2 start,
    held through tick 3.
  - `late_slide_open_mid_row` — `E4 _ >_ /G4`: hold tick 1 + 2,
    slide tick 3, hold G4 tick 4.
- [ ] `just push` green.

## Notes

- This is the breaking change for downstream patches. Run the
  migration script as the first commit of the PR so subsequent
  diffs are reviewable.
- The slide-open-without-close case is a row-build error in this
  ticket. Emit a clear diagnostic pointing at the open cell.
- `/value` does not interact with `value*N` (no `/value*N` form);
  rejected at the grammar level.
- Memory notes worth saving on close: the close rule, the
  continuation absorption asymmetry, and the `value>value` sugar
  equivalence (all called out in the epic's notes section).
