---
id: "0736"
title: Mono→stereo broadcast coercion in cable builder
priority: high
created: 2026-04-27
---

## Summary

When a mono source connects to a stereo input, the planner tags the
cable as broadcast and the consumer's `StereoInput::read()` returns
`(s, s)` reading from the underlying mono slot. No synthetic
broadcaster module, no extra audio-thread work.

## Acceptance criteria

- [ ] Cable builder accepts mono→stereo connections without diagnostic.
- [ ] `StereoInput` resolves a broadcast cable by reading the mono slot
      and replicating; non-broadcast resolves to a poly slot's lanes 0/1.
- [ ] Broadcast tag stored on the cable record (or encoded by routing
      `StereoInput.slot` to the mono slot index with a flag), accessible
      O(1) at tick time.
- [ ] Type checker still rejects stereo→mono and poly↔stereo.
- [ ] Test: a patch wiring a single oscillator into `stereo_delay.in`
      builds and produces identical L/R output.

## Notes

ADR 0059 §2. Broadcast is silent — no warning, no diagnostic, no
diagram node. The broadcast bit is the only consumer-side shape change
in the cable system; document it in the cable module's top comment.
