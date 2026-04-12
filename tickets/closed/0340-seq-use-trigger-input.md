---
id: "0340"
title: Refactor Seq to use TriggerInput for clock/start/stop/reset
priority: medium
created: 2026-04-12
---

## Summary

Replace the four `prev_*: f32` fields and four `MonoInput` fields in `Seq`
with four `TriggerInput` fields (`in_clock`, `in_start`, `in_stop`,
`in_reset`). This is the biggest single boilerplate win — eight fields
become four, and the 8-line read/detect/update block becomes four
single-line `tick` calls.

## Acceptance criteria

- [ ] `prev_clock`, `prev_start`, `prev_stop`, `prev_reset` removed
- [ ] `in_clock`, `in_start`, `in_stop`, `in_reset` changed to `TriggerInput`
- [ ] `process` uses `self.in_clock.tick(pool)` etc.
- [ ] `cargo test -p patches-modules` passes
- [ ] `cargo clippy -p patches-modules` clean

## Notes

Depends on 0335. See ADR 0030. Epic E062.
