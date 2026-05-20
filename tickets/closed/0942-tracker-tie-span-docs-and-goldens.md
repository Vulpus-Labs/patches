---
id: "0942"
title: Manual update + golden audio tests for tie-spread rolls
priority: low
created: 2026-05-19
closed: 2026-05-20
epic: E152
depends_on: ["0940"]
---

## Summary

Document the tie-spread roll in the manual's tracker chapter, and add
a golden audio test that exercises the feature end-to-end through the
audio engine (not just the pattern-player unit tests).

## Acceptance criteria

- [x] `docs/src/dsl-reference.md` gains a "Ties — sustain vs roll
      continuation" section with a sub-trigger offset table for
      `x*3`, `x*3 ~`, `x*3 ~ ~`, `x*5 ~ ~`, and `note ~`, plus
      worked authoring examples and a "Known limitations"
      sub-section covering swing-within-span, row-end truncation,
      cross-loop/bank behaviour, and mid-span live edits.
      `docs/src/modules/tracker.md` cross-references the new
      reference section from the PatternPlayer's "Slides, repeats,
      and tie-spread rolls" sub-heading.
- [x] `patches-integration-tests/tests/tracker/slides_repeats.rs`
      gains three end-to-end tests through the audio engine:
  - `pattern_with_tie_spread_x3_tilde` — drives `x*3 ~ . .`, counts
    trigger pulses across two ticks, asserts exactly 3 triggers
    at expected sample offsets (interval `2T/3`), and that the last
    sub-trigger sits inside the two-tick span.
  - `pattern_with_tie_spread_x5_tilde_tilde` — drives `x*5 ~ ~ . .`,
    asserts exactly 5 triggers at `k * 3T/5` (k=0..4).
  - `pattern_with_plain_tie_sustains_unchanged` — drives
    `A3 ~ . .`, asserts gate has a single rising edge and stays
    high across the tie tick. Regression guard so the new tie
    interpretation does not hijack sustain ties.
- [x] Existing tracker-related tests still pass —
      `pattern_with_repeats` (`x*3` single-tick),
      `repeat_retrigger_audible_through_voice`,
      `repeat_retrigger_audible_with_sustain`,
      `pattern_with_slides`, plus the broader transport / loop /
      pattern-switching suite (`cargo test -p
      patches-integration-tests --test tracker` reports 14 passed).
- [x] `just push` green (build / test / clippy / forbid all clean).
      Unblocked an unrelated pre-existing clippy regression on
      `patches-modules/src/delay/mod.rs` from the recent
      `Module reorg` commit (`module_inception` warning;
      `delay/mod.rs` is the module-group façade, `delay/delay.rs`
      is the `Delay` struct — a deliberate organisation) with a
      one-line `#[allow(clippy::module_inception)]`.

## Resolution

- Manual updates only — no doctests added (per the dev-loop
  memory).
- The "golden audio" wording in the original acceptance bullets was
  reinterpreted: the integration suite is behavioural-assertion
  style, not WAV-byte-equality. Trigger-offset and gate-shape
  assertions through the engine cover the audible side of the
  feature; bit-identity for `x*N` alone is already pinned down by
  `spread_span_1_is_bit_identical_to_pre_e152` in the
  pattern-player core suite (ticket 0940).
- No fusion-Phase-2 golden regeneration was needed; ticket 0849
  hasn't landed yet.

## Notes

- Per the dev-loop memory, no doctests anywhere — the manual examples
  should be plain code fences, not testable snippets.
- The fusion-Phase-2 memory warns that audio goldens may be
  regenerated; if this ticket lands after ticket 0849, regenerate
  goldens *only* for the new tie-spread tests, not for unrelated
  patches.
