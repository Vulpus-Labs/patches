---
id: "0940"
title: Pattern player — spread *N triggers across span samples
priority: medium
created: 2026-05-19
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

- [ ] `x*3` (span=1) produces bit-identical sample-by-sample output to
      pre-change — verified by an explicit regression test recording
      trigger sample indices.
- [ ] `x*3 ~` produces 3 trigger pulses at sample offsets `0`, `T`,
      `2T` where `T = tick_duration_samples * 2 / 3`, and the absorbed
      tie cell fires no independent trigger or gate change at its tick
      rise.
- [ ] `x*5 ~ ~` produces 5 trigger pulses across 3 ticks at offsets
      `k * (tick_duration_samples * 3 / 5)` for `k = 0..5`.
- [ ] Gate articulation (`gate_off_at` = trigger + 0.8 * interval)
      scales with the new interval and the gate falls between
      triggers.
- [ ] If the pattern player is mid-roll when a *non-absorbed* cell
      arrives (e.g. user edits the tie to a note), the roll completes
      its in-flight schedule and the new cell applies on its own tick
      rise. Document this in a test.
- [ ] If the roll's last sub-trigger falls past the end of the span
      (off-by-one paranoia: ensure interval math is `span/N` not
      `(span-1)/(N-1)`), trigger fires inside the span. Test asserts
      the last trigger sample index is `<= span * tick_duration_samples`.
- [ ] If `step_fraction > 0` when entering an `*N` anchor (mid-step
      seek), the existing mid-step roll-entry logic
      ([`pattern_player/mod.rs:226-231`](../../patches-modules/src/tracker_core/pattern_player/mod.rs#L226-L231))
      generalises to the longer interval — write a test.
- [ ] `just commit -p patches-modules -p patches-interpreter` green.

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
