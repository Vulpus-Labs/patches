---
id: "0361"
title: Add TempoSync module (BPM + subdivision to ms)
priority: medium
created: 2026-04-12
---

## Summary

Add a pure-calculator module that takes a BPM input and a beat subdivision parameter and emits the corresponding tick interval in milliseconds. This provides a single reusable source of tempo-synced timing that downstream modules (delays, LFOs) can consume via a sync input, without needing any tempo-awareness in their own implementations.

## Acceptance criteria

- [ ] New `TempoSync` module in `patches-modules`.
- [ ] Mono input `bpm` — tempo in beats per minute (can be wired from `HostTransport` tempo output or set manually).
- [ ] Parameter `subdivision` — enum or fractional selector for common beat divisions (e.g. 1/1, 1/2, 1/4, 1/8, 1/16, dotted variants, triplet variants).
- [ ] Mono output `ms` — tick interval in milliseconds for the chosen subdivision at the current BPM.
- [ ] Stateless: output is a pure function of inputs, no edge detection or clock state.
- [ ] Delay and LFO modules gain a `sync` mono input. When connected, `sync` fully overrides the module's own time/frequency parameter.
- [ ] Module doc comment follows the standard format.
- [ ] Tests covering representative subdivisions and the sync override behaviour in Delay/LFO.
- [ ] `cargo clippy` and `cargo test` pass.

## Notes

The sync input contract is simple: if connected, treat the value as milliseconds and ignore the manual parameter. This keeps DSP modules free of tempo logic. The TempoSync module is the only place that knows about BPM and subdivisions.
