---
id: "0819"
title: Migrate MIDI callers to write_midi_event; retire write_midi
priority: high
created: 2026-05-05
epic: E137
depends_on: "0818"
---

## Summary

Move every MIDI ingest call site to the block-rate API introduced by
ticket 0818, then delete `PatchProcessor::write_midi`,
`midi_overflow`, and the per-sample drain logic. After this ticket,
the per-sample tick loop holds no MIDI event-iteration state.

## Acceptance criteria

### Player

- [ ] Player audio callback drains the SPSC consumer at block boundary
      into `processor.write_midi_event(offset, ev)`. Late arrivals
      (event timestamp earlier than `block_start`) clamp to
      `offset = 0`.
- [ ] `processor.prepare_midi_block(frames)` fires once before the
      per-sample loop, alongside the host-control prepare introduced
      by ticket 0817.
- [ ] `EventQueueConsumer::drain_window` or its caller surface
      adjusted: window is now `[block_start, block_start + frames)`
      with one drain per block instead of per-sample.

### CLAP

- [ ] `patches-clap::plugin::process()` walks `clap_input_events` once
      before the tick loop. For each `CLAP_EVENT_MIDI`, calls
      `write_midi_event(header.time, midi_event)`.
- [ ] `prepare_midi_block(frames)` and `prepare_host_control_block(frames)`
      both fire before the per-sample loop. Per-sample loop is pure DSP.
- [ ] CLAP `params.flush` continues to do nothing for MIDI (the
      extension is host-control parameters only).

### Tests / harnesses

- [ ] Integration tests using `processor.write_midi(events)` migrate
      to `write_midi_event` + `prepare_midi_block`.
- [ ] `dispatch_midi` (patches-engine) — either retire it or rewrite
      it to feed the AoS frame; clarify which inside the ticket.

### Cleanup

- [ ] Remove `PatchProcessor::write_midi`, `midi_overflow`,
      `midi_overflow_count`, and the per-sample overflow drain in
      `tick()`. The MIDI flush in `tick()` reads the AoS row only.
- [ ] Remove `MAX_STASH` if it has no remaining users; otherwise
      document the new sole user.

### Validation

- [ ] `just inner` workspace passes.
- [ ] `just commit -p patches-engine -p patches-modules -p patches-clap -p patches-player`
      passes (clippy clean on touched scope).
- [ ] `just push` passes including determinism / hash-stability suite.
      Hash output must be byte-identical to the pre-migration baseline
      for every fixture in the suite (the AoS frame is a deterministic
      reordering of the same input event list).

## Notes

- The CLAP MIDI ingest path lands here, not in 0817; 0817 stays
  scoped to host control. Both end up sharing the same
  "single sweep before tick" shape.
- `dispatch_midi` may simply become a thin caller of
  `write_midi_event` in a loop and lose its independent existence.
  Decide as part of the migration; either is fine as long as the
  callers don't observe regressions.
- Keep the player's SPSC drain interval the same (one block) — no
  latency change vs. the current per-sample drain, just the same
  events stamped at the same offsets via a different API.
