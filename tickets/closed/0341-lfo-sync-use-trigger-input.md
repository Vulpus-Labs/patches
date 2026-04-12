---
id: "0341"
title: Refactor LFO sync to use TriggerInput and standard 0.5 threshold
priority: medium
created: 2026-04-12
---

## Summary

The LFO sync input currently uses `MonoInput` with a non-standard threshold
(`<= 0.0` / `> 0.0`) for edge detection. Replace with `TriggerInput` to
standardise on the 0.5 threshold used everywhere else.

## Acceptance criteria

- [ ] `prev_sync` removed from `Lfo`
- [ ] Sync input changed to `TriggerInput`
- [ ] Phase reset uses `self.in_sync.tick(pool)` with standard 0.5 threshold
- [ ] `cargo test -p patches-modules` passes
- [ ] `cargo clippy -p patches-modules` clean

## Notes

Depends on 0335. See ADR 0030. Epic E062.
