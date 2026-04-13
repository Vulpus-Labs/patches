---
id: "0362"
title: Add MsTicker module (ms interval to trigger pulses)
priority: medium
created: 2026-04-12
---

## Summary

Add a module that emits trigger pulses at a given millisecond interval. This complements TempoSync (0361) by converting a tempo-synced ms value into a clock signal that can drive sample-and-hold, sequencer-style stepping, or any other trigger-driven module.

## Acceptance criteria

- [ ] New `MsTicker` module in `patches-modules`.
- [ ] Mono input `ms` — tick interval in milliseconds (can be wired from TempoSync output or set manually).
- [ ] Mono input `reset` — rising edge resets the internal phase to zero, so the next tick fires immediately. Use to sync to song start, bar boundaries, etc.
- [ ] Mono output `trigger` — single-sample pulse (1.0) at each interval boundary, 0.0 otherwise.
- [ ] Mono output `gate` — high for the first half of each interval, low for the second half (square wave at the tick rate).
- [ ] Handles dynamic changes to `ms` input smoothly (adjusts time-to-next-tick without glitching).
- [ ] Module doc comment follows the standard format.
- [ ] Tests covering steady-state ticking, dynamic interval changes, and edge cases (very short / very long intervals).
- [ ] `cargo clippy` and `cargo test` pass.

## Notes

Together with TempoSync, this provides a tempo-synced clock source for arbitrary use: `HostTransport.tempo -> TempoSync(1/8) -> MsTicker -> S&H`, etc.
