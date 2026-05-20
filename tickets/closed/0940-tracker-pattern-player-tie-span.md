---
id: "0940"
title: Pattern player — spread *N triggers across span samples
priority: medium
created: 2026-05-19
closed: 2026-05-19
epic: E152
depends_on: ["0939"]
---

## Summary

Consume the `repeat_span` annotation from ticket 0939. On a `*N`
anchor with `span > 1`, compute the roll interval over the full span
duration instead of a single tick, and ignore subsequent absorbed-tie
cells while the in-flight schedule completes.

Today
([`pattern_player/mod.rs:223`](../../patches-modules/src/tracker_core/pattern_player/mod.rs#L223)):

```rust
let interval = self.current_tick_duration_samples / step.repeat as f32;
```

New:

```rust
let span = step.repeat_span.max(1) as f32;
let interval = (self.current_tick_duration_samples * span) / step.repeat as f32;
```

When the next tick rises during the span, the `apply_step` call for
that tick's cell must:

1. Detect that the cell is `absorbed_by_roll` and **not** re-run
   slide/repeat/trigger logic on it.
2. Update `current_tick_duration_samples` (the sequencer's authority on
   tick length) **without** resetting the in-flight roll's
   `repeat_samples_elapsed` or `repeat_interval_samples`. The roll
   keeps its anchor-tick interval (v1; per-tick recomputation is
   epic-out-of-scope).
3. Keep the trigger-clear / activity-count bookkeeping coherent. The
   roll is still counted in `repeat_active_count` until it finishes
   inside the span.

## Acceptance criteria

- [x] `x*3` (span=1) — `spread_span_1_is_bit_identical_to_pre_e152`
      asserts triggers at samples `0, 100, 200` (300-sample tick),
      identical to pre-E152 behaviour.
- [x] `x*3 ~` — `spread_x3_tie_three_triggers_across_two_ticks`
      asserts triggers at `0, 200, 400`
      (`T = tick_duration_samples * 2 / 3 = 200`). The absorbed
      tie tick rise emits no fresh trigger
      (`spread_absorbed_tie_does_not_fire_independent_trigger`).
- [x] `x*5 ~ ~` — `spread_x5_tie_tie_five_triggers_across_three_ticks`
      asserts triggers at `0, 180, 360, 540, 720`
      (`T = tick_duration_samples * 3 / 5 = 180`) and that the last
      trigger sample sits strictly inside the span
      (`< 3 * tick_duration_samples = 900`).
- [x] Gate articulation —
      `spread_gate_articulation_scales_with_longer_interval`
      drives `x*3 ~` and asserts gate high at sample 159, low at 160
      (`0.8 * interval = 160`), high again at 200 with the next sub.
- [x] Mid-roll live edit —
      `spread_non_absorbed_cell_completes_in_flight_roll_at_its_boundary`
      swaps the tracker between ticks, demonstrating that a
      newly-non-absorbed cell takes over its tick and the prior
      roll is replaced.
- [x] Mid-step entry — `spread_mid_step_entry_uses_span_interval`
      enters the anchor at `step_fraction=0.25` and asserts the
      schedule's interval = 200 and elapsed = 75 (= 0.25 * tick).
- [x] `just inner -p patches-modules -p patches-interpreter` green.

## Resolution

- `PatternPlayerCore::apply_step`:
  - Short-circuits at the top when `step.absorbed_by_roll` is true.
    For channels with `repeat_active` it still advances the in-flight
    roll by one sample so the rising-edge sample counts toward the
    schedule — otherwise the roll drifts +1 sample per absorbed
    tick.
  - The repeat-arm interval formula is now
    `(current_tick_duration_samples * span) / repeat`, where
    `span = step.repeat_span.max(1)`. `span = 1` collapses to the
    pre-E152 single-tick formula, preserving bit-identity for
    non-spread rolls.
- Per-sample roll advance was extracted to
  `PatternPlayerCore::advance_roll_one_sample(ch)`. Both the
  inter-tick loop and the absorbed-tie path in `apply_step` use it,
  so the gate-off / sub-trigger / "roll done" bookkeeping has one
  authoritative implementation.
- The activity recount loop already reconciles `repeat_active_count`
  from per-channel flags, so an absorbed-tie advance that completes
  a roll mid-tick is handled correctly without extra work in
  `tick()`.

## Notes

- The `apply_step` short-circuit on absorbed-tie cells should still
  update `step_index[ch]` so the LSP and any external introspection
  sees the row advancing correctly.
- Swing across the span is accepted as audibly-slightly-wrong in v1
  (epic-level note). Don't try to "fix" it here unless a
  golden-test failure makes it unavoidable.
- Pattern-loop boundary: if the row-build layer truncates `repeat_span`
  at row end (ticket 0939), the pattern player doesn't need extra
  logic; the roll just finishes within the truncated span using
  `triggers = floor(span_samples / interval) + 1`. Decide whether to
  cap triggers at `N` (drop overflow) or scale interval to fit
  truncated span — recommend cap-at-N (truthful to authored `*N`).
