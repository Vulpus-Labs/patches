---
id: "0945"
title: Per-channel sub-event schedule respecting per-tick swung durations
priority: medium
created: 2026-05-20
closed: 2026-05-20
epic: E153
depends_on: ["0944"]
---

## Summary

Replace the single `repeat_interval_samples` capture with a
per-channel **sub-event schedule**: a small vector of `(tick_index,
fraction_within_tick)` pairs. The pattern player consumes one
sub-event per inter-tick advance when the current tick's elapsed
samples cross `fraction * current_tick_duration_samples` — using the
*current* tick's swung duration, not the anchor tick's.

This resolves E152's documented v1 swing-within-span limitation
(the "anchor-tick interval used for the whole span" note in the
epic). Swung patterns get audibly correct sub-trigger placement
even when the span crosses a swing boundary.

For E152's `value*N _ _` patterns this means:
- Non-swung: bit-identical to 0940's behaviour.
- Swung: sub-triggers land at the correct *clock time*, not the
  uniform-interval approximation. Goldens for swung patterns
  shift by sub-sample amounts and are regenerated in this ticket.

The same scheduling mechanism is used for multi-tick slides
introduced in ticket 0946 (one ramp segment per slide tick, with
each segment's sample-time placement resolved using the current
tick's swung duration). The schedule is the unifying mechanism;
this ticket lands it for rolls and prepares the infrastructure for
slides.

## Acceptance criteria

- [ ] `PatternPlayerCore` gains a per-channel
      `Vec<SubEvent>` (allocated to capacity 16 in `new`, no
      audio-thread alloc). `SubEvent { tick_idx_in_span: u8,
      fraction: f32 }`.
- [ ] When `apply_step` resolves a `StartNote` with `roll: Some(_)`,
      it computes the schedule: `t_k = k / N * S` for `k = 0..N-1`,
      split into `(tick_idx = floor(t_k), fraction = t_k -
      floor(t_k))`. Anchor sub-event is consumed immediately
      (fires inline); remaining pairs are queued.
- [ ] The inter-tick `advance_roll_one_sample` advance now reads
      the head of the schedule, compares
      `elapsed_in_current_tick >= fraction * current_tick_dur`,
      and fires + pops the head when crossed.
- [ ] On an absorbed-roll tick rise: `current_tick_dur` is updated
      from the clock bus; `tick_idx_in_span` for the head sub-event
      is decremented (it now refers to the *current* tick, not a
      future one). Schedule logic continues with the new duration.
- [ ] Tests:
  - `non_swung_x3_tilde_bit_identical_to_e152` — drive the same
    `x*3 ~` pattern with `swing = 0.5`; trigger sample offsets
    match 0940's `spread_x3_tie_three_triggers_across_two_ticks`
    exactly.
  - `non_swung_x3_alone_bit_identical_to_pre_e152` — `x*3` alone
    at `swing = 0.5`; trigger offsets at samples `0`, `T/3`,
    `2T/3`.
  - `swung_x3_tilde_uses_per_tick_durations` — `x*3 ~` at e.g.
    `swing = 0.66`; sub-trigger 2 falls inside the lengthened
    even tick, sub-trigger 3 inside the shortened odd tick. Each
    sub-trigger's sample offset = (anchor offset within tick) +
    (per-tick durations summed). Assert exact sample positions
    against hand-computed expected values.
  - `swung_x5_tilde_tilde_per_tick_durations` — analogue for
    `x*5 ~ ~` across three ticks.
- [ ] Manual regeneration of swung audio goldens for E152 patterns
      (only the swung ones; non-swung stays bit-identical). The
      regenerated WAV files are committed; their checksums are
      noted in this ticket's resolution.
- [ ] All non-swung tracker integration tests pass unchanged.
- [ ] `just push` green.

## Notes

- The schedule structure is also what ticket 0946 will use for
  multi-tick slides. Provision the `SubEvent` representation now to
  cover both roll sub-triggers (point-in-time events) and slide
  segments (start/end pairs). A unified `SubEvent` may want a
  `kind: SubEventKind` tag — design decision for the implementer.
- Memory note on close: `repeat_interval_samples` is gone; the
  schedule is authoritative. Future tickets touching tracker timing
  should reason about the schedule, not interval captures.
- Sub-events allocate `Vec<SubEvent>` per channel at construction;
  reset (don't reallocate) on each new `StartNote` with rolls.
  Realtime-safe.

## Resolution

Landed. `PatternPlayerCore` carries `sub_events: Vec<Vec<SubEvent>>`
(preallocated to `SUB_EVENT_CAPACITY = 16` per channel) and a
`sub_event_head: Vec<usize>` cursor. `apply_step` for
`StartNote { roll: Some(_) }` clears the channel's schedule and
pushes one entry per `k = 0..N-1` with `t_k = k / N * span` split
into `(floor, frac)`; the anchor (k = 0) is consumed inline and
the cursor starts at 1. The inter-tick path's `advance_roll_one_sample`
ticks the gate-off countdown and calls a new `check_sub_event_fire`
helper, which fires the head when `current_tick_elapsed_samples >=
head.fraction * current_tick_duration_samples`. On an absorbed-roll
tick rise we update `current_tick_duration_samples` from the bus,
decrement every unfired sub-event's `tick_idx_in_span` (cap-16 vec —
trivial), and call `check_sub_event_fire` once to catch fraction-0
boundary firings.

Cross-tick gate-off distances approximate with the current tick
duration, so non-swung patterns stay bit-identical with the prior
formula. New unit tests cover the swung path:

- `non_swung_x3_tilde_bit_identical_to_e152`
- `non_swung_x3_alone_bit_identical_to_pre_e152`
- `swung_x3_tilde_uses_per_tick_durations` (396/204 ticks; triggers
  at samples `[0, 264, 464]`)
- `swung_x5_tilde_tilde_per_tick_durations` (396/204/396 ticks;
  triggers at `[0, 238, 437, 560, 759]`)

E152 had no committed swung audio goldens — the integration suite's
`pattern_with_tie_spread_*` tests run at `swing = 0.5` (uniform
ticks), so non-swung remains bit-identical and no WAV regeneration
was required. `cargo test -p patches-modules` (458 unit tests) and
`cargo test -p patches-integration-tests --test tracker` (14 tests)
both green.

Memory note saved: [`project_sub_event_schedule.md`] — schedule is
authoritative; future tracker timing work reasons about it, not about
interval captures.
