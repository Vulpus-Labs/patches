---
id: "0324"
title: "MasterSequencer module"
priority: high
created: 2026-04-11
---

## Summary

Implement the `MasterSequencer` module in patches-modules. It drives song
playback by reading the song order from `TrackerData` and outputting a
poly clock bus per song channel.

## Acceptance criteria

- [ ] Module registered as `MasterSequencer` in the module registry
- [ ] Shape arg: `channels` (list of channel aliases)
- [ ] Parameters: `bpm` (float), `rows_per_beat` (int), `song` (string),
      `loop` (bool, default true), `autostart` (bool, default true),
      `swing` (float, 0.0–1.0, default 0.5)
- [ ] Mono inputs: `start`, `stop`, `pause`, `resume` (trigger-receiving)
- [ ] Poly outputs: `clock` (one per channel alias via `poly_out_multi`)
- [ ] Clock bus voices: 0=pattern reset, 1=pattern bank index,
      2=tick trigger, 3=tick duration
- [ ] Implements `ReceivesTrackerData` — looks up its song by the `song`
      parameter name
- [ ] Transport: `autostart` begins playback on activation; `start`
      resets and plays; `stop` halts and resets; `pause` halts in place;
      `resume` continues
- [ ] Swing: alternating step durations via `2 * base_tick * swing` /
      `2 * base_tick * (1 - swing)`
- [ ] End-of-song: if `loop` is true, jump to `loop_point`; if false,
      stop emitting ticks and send stop sentinel (bank index -1)
- [ ] Doc comment follows module documentation standard
- [ ] Unit tests: tick timing at various BPMs, swing, transport state
      machine, loop point, end-of-song behaviour
- [ ] `cargo test -p patches-modules` passes
- [ ] `cargo clippy -p patches-modules` clean

## Notes

The MasterSequencer is sample-accurate — it counts samples against the
tick duration to determine when to advance. Tick duration varies per step
when swing is applied.

The poly clock bus encoding keeps PatternPlayers decoupled from BPM and
swing — they just see per-tick durations and pattern indices.

Epic: E060
ADR: 0029
